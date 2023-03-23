use core::fmt::Debug;

use alloc::{
    borrow::ToOwned,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use bitflags::bitflags;
use log::warn;
use spin::Once;

use crate::{
    errno,
    mem::addr::VirtAddr,
    task::{current_task, get_scheduler, group::TaskGroup, signal::SIGINT, wait_queue::WaitQueue},
    userland::buffer::{UserBuffer, UserBufferMut, UserBufferReader, UserBufferWriter},
    util::{ctypes::c_int, errno::Errno, error::KResult, lock::IrqMutex, ringbuffer::RingBuffer},
};

use super::{
    initramfs::get_root, opened_file::OpenOptions, path::Path, File, FsNode, INode, PollStatus,
    Stat, S_IFCHR, POLL_WAIT_QUEUE,
};

pub static TTY: Once<Arc<Tty>> = Once::new();

pub fn init() {
    TTY.call_once(|| Arc::new(Tty::new("tty")));
    get_root()
        .unwrap()
        .lookup(Path::new("dev"), true)
        .unwrap()
        .as_dir()
        .unwrap()
        .insert(INode::File(TTY.get().unwrap().clone()));
}

bitflags! {
    // #[derive(Default)]
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct LFlag: u32 {
        const ICANON = 0o0000002;
        const ECHO   = 0o0000010;
    }
}

bitflags! {
    // #[derive(Default)]
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct IFlag: u32 {
        const IGNBRK	= 0o0000001;
        const BRKINT	= 0o0000002;
        const IGNPAR	= 0o0000004;
        const PARMRK	= 0o0000010;
        const INPCK	    = 0o0000020;
        const ISTRIP	= 0o0000040;
        const INLCR	    = 0o0000100;
        const IGNCR	    = 0o0000200;
        const ICRNL	    = 0o0000400;
        const IUCLC	    = 0o0001000;
        const IXON	    = 0o0002000;
        const IXANY	    = 0o0004000;
        const IXOFF	    = 0o0010000;
        const IMAXBEL	= 0o0020000;
        const IUTF8	    = 0o0040000;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    iflag: IFlag,
    oflag: u32,
    cflag: u32,
    lflag: LFlag,
    cc: [u8; 0],
    reserved: [u32; 3],
    ispeed: u32,
    ospeed: u32,
}

impl Termios {
    pub fn is_cooked(&self) -> bool {
        self.lflag.contains(LFlag::ICANON)
    }
}

impl Default for Termios {
    fn default() -> Self {
        Termios {
            iflag: IFlag::ICRNL,
            lflag: LFlag::ICANON | LFlag::ECHO,

            oflag: 0,
            cflag: 0,
            cc: [0; 0],
            reserved: [0; 3],
            ispeed: 0,
            ospeed: 0,
        }
    }
}

impl Debug for Termios {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Termios")
            .field("iflag", &self.iflag)
            .field("lflag", &self.lflag)
            .finish()
    }
}

pub enum LineControl {
    Backspace,
    Echo(u8),
}

pub struct LineDiscipline {
    termios: IrqMutex<Termios>,
    wait_queue: WaitQueue,
    current_line: IrqMutex<Vec<u8>>,
    buf: IrqMutex<RingBuffer<u8, 4096>>,
    foreground_process_group: IrqMutex<Weak<IrqMutex<TaskGroup>>>,
}

impl Default for LineDiscipline {
    fn default() -> Self {
        Self::new()
    }
}

impl LineDiscipline {
    pub fn new() -> LineDiscipline {
        LineDiscipline {
            termios: IrqMutex::new(Termios::default()),
            foreground_process_group: IrqMutex::new(Weak::new()),
            wait_queue: WaitQueue::new(),
            buf: IrqMutex::new(RingBuffer::new()),
            current_line: IrqMutex::new(Vec::new()),
        }
    }

    pub fn is_readable(&self) -> bool {
        self.buf.lock().is_readable()
    }

    pub fn is_writable(&self) -> bool {
        self.buf.lock().is_writable()
    }

    pub fn foreground_process_group(&self) -> Option<Arc<IrqMutex<TaskGroup>>> {
        self.foreground_process_group.lock().upgrade()
    }

    pub fn set_foreground_process_group(&self, pg: Weak<IrqMutex<TaskGroup>>) {
        *self.foreground_process_group.lock() = pg;
    }

    pub fn is_current_foreground(&self) -> bool {
        let fg = &*self.foreground_process_group.lock();
        let current = current_task();
        current.belongs_to_group(fg) || fg.upgrade().is_none()
    }

    pub fn write<F: Fn(LineControl)>(&self, buf: UserBuffer<'_>, callback: F) -> KResult<usize> {
        let termios = self.termios.lock();
        let mut reader = UserBufferReader::from(buf);
        let mut current_line = self.current_line.lock();
        let mut ringbuf = self.buf.lock();
        let mut written_len = 0;
        while reader.remaining_len() > 0 {
            let mut tmp = [0; 128];
            let copied_len = reader.read_bytes(&mut tmp)?;
            for ch in &tmp.as_slice()[..copied_len] {
                match ch {
                    // ctrl-c
                    0x03 => {
                        if termios.is_cooked() {
                            if let Some(pg) = self.foreground_process_group() {
                                pg.lock().signal(SIGINT);
                            }
                        }
                    }
                    b'\r' => {
                        // if termios.iflag.contains(IFlag::ICRNL) {
                            // current_line.push(b'\r');
                            current_line.push(b'\n');
                            ringbuf.push(b'\n').ok();
                            // serial1_println!();
                            // ringbuf.push_slice(current_line.as_slice());
                            current_line.clear();
                            if termios.lflag.contains(LFlag::ECHO) {
                                // callback(LineControl::Echo(b'\r'));
                                callback(LineControl::Echo(b'\n'));
                            }
                        // }
                    }
                    b'\n' => {
                        // current_line.push(b'\r');
                        current_line.push(b'\n');
                        // vga_print!("\n");
                        // serial1_println!();
                        ringbuf.push(b'\n').ok();
                        // ringbuf.push_slice(current_line.as_slice());
                        current_line.clear();
                        if termios.lflag.contains(LFlag::ECHO) {
                            // callback(LineControl::Echo(b'\r'));
                            callback(LineControl::Echo(b'\n'));
                        }
                    }
                    // backspace
                    0x7f | 0x08 if termios.is_cooked() => {
                        if !current_line.is_empty() {
                            // vga_print!("\x08 \x08");
                            current_line.pop();
                            callback(LineControl::Backspace);
                        }
                    }
                    ch if 0x20 <= *ch && *ch <= 0x7f && termios.is_cooked() => {
                        current_line.push(*ch);
                        ringbuf.push(*ch).ok();
                        if termios.lflag.contains(LFlag::ECHO) {
                            callback(LineControl::Echo(*ch));
                        }
                    }
                    _ => {
                        ringbuf.push(*ch).ok();
                    }
                }

                written_len += 1;
            }
        }

        if written_len > 0 {
            get_scheduler().wake_all(&self.wait_queue);
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }
        Ok(written_len)
    }

    fn read(&self, buf: UserBufferMut) -> KResult<usize> {
        let mut writer = UserBufferWriter::from(buf);
        let read_len = self.wait_queue.sleep_signalable_until(None, || {
            if !self.is_current_foreground() {
                return Ok(None);
            }

            let mut buf_lock = self.buf.lock();
            while writer.remaining_len() > 0 {
                if let Some(slice) = buf_lock.pop_slice(writer.remaining_len()) {
                    writer.write_bytes(slice)?;
                } else {
                    break;
                }
            }

            if writer.written_len() > 0 {
                Ok(Some(writer.written_len()))
            } else {
                Ok(None)
            }
        })?;
        if read_len > 0 {
            get_scheduler().wake_all(&self.wait_queue);
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }
        Ok(read_len)
    }
}

pub struct Tty {
    discipline: LineDiscipline,
    name: String,
}

impl Default for Tty {
    fn default() -> Self {
        Self::new("tty")
    }
}

impl Tty {
    pub fn new(name: &str) -> Tty {
        Tty {
            name: name.to_owned(),
            discipline: LineDiscipline::new(),
        }
    }

    pub fn input_char(&self, ch: u8) {
        self.discipline
            .write(UserBuffer::from_slice(&[ch]), |ctrl| match ctrl {
                LineControl::Echo(ch) => {
                    self.write(0, UserBuffer::from_slice(&[ch]), &OpenOptions::readwrite())
                        .ok();
                }
                LineControl::Backspace => {
                    serial1_print!("\x08 \x08");
                    // serial_print!("\x08 \x08");
                }
            })
            .ok();
    }

    pub fn set_foreground_process_group(&self, pg: Weak<IrqMutex<TaskGroup>>) {
        self.discipline.set_foreground_process_group(pg);
    }
}

impl File for Tty {
    fn ioctl(&self, cmd: usize, arg: usize) -> KResult<isize> {
        // const TIOCSPTLCK: usize = 0x40045431;
        const TCGETS: usize = 0x5401;
        const TCSETS: usize = 0x5402;
        const TIOCGPGRP: usize = 0x540f;
        const TIOCSPGRP: usize = 0x5410;
        const TIOCGWINSZ: usize = 0x5413;

        match cmd {
            // TIOCSPTLCK => Ok(0),
            TCGETS => {
                let arg = VirtAddr::new(arg);
                arg.write(&Termios {
                    lflag: LFlag::all(),
                    iflag: IFlag::all(),
                    ..Default::default()
                })?;
            }
            TCSETS => {
                let arg = VirtAddr::new(arg);
                let termios = arg.read::<Termios>()?;
                let mut lock = self.discipline.termios.lock();
                *lock = *termios;
                // lock.iflag = termios.iflag;
            }
            TIOCGPGRP => {
                let group = self
                    .discipline
                    .foreground_process_group()
                    .ok_or_else(|| errno!(Errno::ENOENT, "ioctl(): no foreground process group set for tty"))?;
                let pgid = group.lock().pgid();
                let arg = VirtAddr::new(arg);

                arg.write(&pgid)?;
            }
            TIOCSPGRP => {
                let arg = VirtAddr::new(arg);
                let pgid = *arg.read::<c_int>()?;
                let pg = get_scheduler()
                    .find_group(pgid)
                    .ok_or_else(|| errno!(Errno::ESRCH, "ioctl(): cannot find group for tty"))?;
                self.discipline
                    .set_foreground_process_group(Arc::downgrade(&pg));
            }
            TIOCGWINSZ => {}
            _ => {
                warn!("ioctl(): unknown cmd: {:#x}", cmd);
                return Err(errno!(Errno::ENOSYS, "ioctl(): unknown cmd"));
            }
        }

        Ok(0)
    }

    fn poll(&self) -> KResult<PollStatus> {
        let mut status = PollStatus::POLLIN;
        // let mut status = PollStatus::empty();
        // if !self.discipline.current_line.lock().is_empty() {
        //     status |= PollStatus::POLLIN;
        // }
        if self.discipline.is_writable() {
            status |= PollStatus::POLLOUT;
        }
        Ok(status)
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(Stat {
            inode_no: 3,
            mode: super::FileMode::new(S_IFCHR | 0o666),
            ..Stat::zeroed()
        })
    }

    fn read(
        &self,
        _offset: usize,
        buf: UserBufferMut,
        _options: &OpenOptions,
        // len: usize,
    ) -> KResult<usize> {
        self.discipline.read(buf)
    }

    fn write(&self, _offset: usize, buf: UserBuffer<'_>, _options: &OpenOptions) -> KResult<usize> {
        let mut tmp = [0; 32];
        let mut total_len = 0;
        let mut reader = UserBufferReader::from(buf);
        while reader.remaining_len() > 0 {
            // get_scheduler().with_kernel_addr_space_active(|| {
            let copied_len = reader.read_bytes(&mut tmp)?;
            serial1_print!("{}", String::from_utf8_lossy(&tmp.as_slice()[..copied_len]));
            total_len += copied_len;
            // Ok(())
            // })?;
        }
        Ok(total_len)
    }
}

impl FsNode for Tty {
    fn get_name(&self) -> String {
        self.name.clone()
    }
}
