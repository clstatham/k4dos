use core::fmt::Debug;

use alloc::{
    borrow::ToOwned,
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use bitflags::bitflags;
use spin::Once;

use crate::{
    errno, fb_print,
    graphics::{self, render_text_buf},
    mem::addr::VirtAddr,
    task::{current_task, get_scheduler, group::TaskGroup, signal::SIGINT, wait_queue::WaitQueue},
    userland::buffer::{UserBuffer, UserBufferMut, UserBufferReader, UserBufferWriter},
    util::{
        ctypes::c_int, errno::Errno, error::KResult, lock::IrqMutex, ringbuffer::RingBuffer, KError,
    },
    vga_text,
};

use crate::fs::{
    initramfs::{dir::InitRamFsDir, get_root},
    opened_file::OpenFlags,
    path::Path,
    File, FileMode, FileRef, FsNode, INode, PollStatus, Stat, POLL_WAIT_QUEUE, S_IFCHR,
};

pub static TTY: Once<Arc<Tty>> = Once::new();

pub fn init() {
    let tty = Arc::new(Tty::new("tty"));
    TTY.call_once(|| tty.clone());
    get_root()
        .unwrap()
        .lookup(Path::new("dev"), true)
        .unwrap()
        .as_dir()
        .unwrap()
        .insert(INode::File(tty));
}

bitflags! {
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct LFlag: u32 {
        const ICANON = 0o0000002;
        const ECHO   = 0o0000010;
    }
}

impl Default for LFlag {
    fn default() -> Self {
        Self::all()
    }
}

bitflags! {
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

impl Default for IFlag {
    fn default() -> Self {
        Self::ICRNL
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
    wait_queue: WaitQueue,
    current_line: IrqMutex<Vec<u8>>,
    buf: IrqMutex<RingBuffer<u8, 4096>>,
    termios: IrqMutex<Termios>,
    foreground_group: IrqMutex<Weak<IrqMutex<TaskGroup>>>,
}

impl LineDiscipline {
    pub fn new() -> Self {
        Self {
            wait_queue: WaitQueue::new(),
            current_line: IrqMutex::new(Vec::new()),
            buf: IrqMutex::new(RingBuffer::new()),
            termios: IrqMutex::new(Termios::default()),
            foreground_group: IrqMutex::new(Weak::new()),
        }
    }

    pub fn is_readable(&self) -> bool {
        self.buf.lock().is_readable()
    }

    pub fn is_writable(&self) -> bool {
        self.buf.lock().is_writable()
    }

    pub fn foreground_group(&self) -> Option<Arc<IrqMutex<TaskGroup>>> {
        self.foreground_group.lock().upgrade()
    }

    pub fn set_foreground_group(&self, pg: Weak<IrqMutex<TaskGroup>>) {
        *self.foreground_group.lock() = pg;
    }

    fn _is_current_foreground(&self) -> bool {
        let pg = &*self.foreground_group.lock();
        current_task().belongs_to_group(pg) || pg.upgrade().is_none()
    }

    pub fn write<F>(&self, buf: UserBuffer<'_>, callback: F) -> KResult<usize>
    where
        F: Fn(LineControl),
    {
        let termios = self.termios.lock();
        let mut current_line = self.current_line.lock();
        let mut ringbuf = self.buf.lock();
        let mut written_len = 0;
        let mut reader = UserBufferReader::from(buf);
        while reader.remaining_len() > 0 {
            let mut tmp = [0; 1];
            let copied_len = reader.read_bytes(&mut tmp)?;
            for ch in &tmp.as_slice()[..copied_len] {
                match ch {
                    0x03 if termios.is_cooked() => {
                        if let Some(pg) = self.foreground_group() {
                            pg.lock().signal(SIGINT);
                        }
                    }
                    0x08 if termios.is_cooked() => {
                        if !current_line.is_empty() {
                            current_line.pop();
                            callback(LineControl::Backspace);
                        }
                    }
                    b'\r' if termios.iflag.contains(IFlag::ICRNL) => {
                        current_line.push(b'\n');
                        ringbuf.push_slice(&current_line);
                        current_line.clear();
                        if termios.lflag.contains(LFlag::ECHO) {
                            callback(LineControl::Echo(b'\r'));
                            callback(LineControl::Echo(b'\n'));
                        }
                    }
                    b'\n' => {
                        current_line.push(b'\n');
                        ringbuf.push_slice(&current_line);
                        current_line.clear();
                        if termios.lflag.contains(LFlag::ECHO) {
                            callback(LineControl::Echo(b'\n'));
                        }
                    }
                    ch if termios.is_cooked() => {
                        if 0x20 <= *ch && *ch < 0x7f {
                            current_line.push(*ch);
                            if termios.lflag.contains(LFlag::ECHO) {
                                callback(LineControl::Echo(*ch));
                            }
                        }
                    }
                    _ => {
                        ringbuf.push(*ch).ok();
                    }
                }

                written_len += 1;
            }
        }

        get_scheduler().wake_all(&self.wait_queue);

        Ok(written_len)
    }

    pub fn read(&self, dst: UserBufferMut<'_>, options: &OpenFlags) -> KResult<usize> {
        let mut writer = UserBufferWriter::from(dst);
        let timeout = if options.contains(OpenFlags::O_NONBLOCK) {
            Some(0)
        } else {
            None
        };
        self.wait_queue.sleep_signalable_until(timeout, || {
            // todo: figure out how to get this working
            // if !self.is_current_foreground() {
            //     return Ok(None)
            // }

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
        })
    }
}

impl Default for LineDiscipline {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Tty {
    name: String,
    discipline: LineDiscipline,
}

impl Tty {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            discipline: LineDiscipline::new(),
        }
    }

    pub fn set_cooked_mode(&self, cooked: bool) {
        if cooked {
            self.discipline.termios.lock().lflag |= LFlag::ICANON | LFlag::ECHO;
        } else {
            self.discipline.termios.lock().lflag &= !(LFlag::ICANON | LFlag::ECHO);
        }
    }

    pub fn input_char(&self, ch: u8) {
        self.discipline
            .write(UserBuffer::from_slice(&[ch]), |ctrl| match ctrl {
                LineControl::Backspace => {
                    // serial1_print!("\x08 \x08");
                    graphics::backspace();
                }
                LineControl::Echo(ch) => {
                    self.write(0, UserBuffer::from_slice(&[ch]), &OpenFlags::empty())
                        .ok();
                }
            })
            .ok();
    }

    pub fn set_foreground_group(&self, pg: Weak<IrqMutex<TaskGroup>>) {
        self.discipline.set_foreground_group(pg)
    }
}

impl FsNode for Tty {
    fn get_name(&self) -> String {
        self.name.clone()
    }
}
const TCGETS: usize = 0x5401;
const TCSETS: usize = 0x5402;
const TCSETSW: usize = 0x5403;

const TIOCGPGRP: usize = 0x540f;
const TIOCSPGRP: usize = 0x5410;
const TIOCGWINSZ: usize = 0x5413;

#[repr(C)]
#[derive(Copy, Clone)]
struct WinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

impl File for Tty {
    fn ioctl(&self, cmd: usize, arg: usize) -> KResult<isize> {
        match cmd {
            TCGETS => {
                let termios = *self.discipline.termios.lock();
                let arg = VirtAddr::new(arg);
                arg.write_volatile(termios)?;
            }
            TCSETS | TCSETSW => {
                let arg = VirtAddr::new(arg);
                let termios = arg.read_volatile::<Termios>()?;
                log::debug!("Termios: {:?}", termios);
                *self.discipline.termios.lock() = termios;
            }
            TIOCGPGRP => {
                // let group = self.discipline.foreground_group().ok_or(errno!(
                //     Errno::ENOENT,
                //     "ioctl(): no foreground process group for tty"
                // ))?;
                let group = self
                    .discipline
                    .foreground_group()
                    .unwrap_or(current_task().group.borrow().upgrade().unwrap());
                let id = group.lock().pgid();
                let arg = VirtAddr::new(arg);
                arg.write_volatile(id)?;
            }
            TIOCSPGRP => {
                let arg = VirtAddr::new(arg);
                let pgid = arg.read::<c_int>()?;
                let pg = get_scheduler().find_or_create_group(*pgid);
                self.discipline.set_foreground_group(Arc::downgrade(&pg));
            }
            TIOCGWINSZ => {
                let winsize = WinSize {
                    ws_row: vga_text::BUFFER_HEIGHT as u16,
                    ws_col: vga_text::BUFFER_WIDTH as u16,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                let arg = VirtAddr::new(arg);
                arg.write_volatile(winsize)?;
            }
            _ => return Err(errno!(Errno::ENOSYS, "ioctl(): command not found")),
        }

        Ok(0)
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(Stat {
            inode_no: 3,
            mode: FileMode::new(S_IFCHR | 0o666),
            ..Stat::zeroed()
        })
    }

    fn read(&self, _offset: usize, buf: UserBufferMut, options: &OpenFlags) -> KResult<usize> {
        let read_len = self.discipline.read(buf, options);
        if let Ok(read_len) = read_len {
            if read_len > 0 {
                get_scheduler().wake_all(&POLL_WAIT_QUEUE);
            }
            Ok(read_len)
        } else if matches!(
            read_len,
            Err(KError::Errno {
                errno: Errno::EINTR,
                ..
            })
        ) && options.contains(OpenFlags::O_NONBLOCK)
        {
            Ok(0)
        } else {
            read_len
        }
    }

    fn write(&self, _offset: usize, buf: UserBuffer<'_>, _options: &OpenFlags) -> KResult<usize> {
        // let mut tmp = [0; 1];
        // let mut total_len = 0;
        let reader = UserBufferReader::from(buf);
        let total_len = parse(reader)?;
        if total_len > 0 {
            render_text_buf();
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }
        Ok(total_len)
    }

    fn poll(&self) -> KResult<PollStatus> {
        let mut status = PollStatus::empty();
        // if self.discipline.is_readable() {
        status |= PollStatus::POLLIN;
        // }
        // if self.discipline.is_writable() {
        status |= PollStatus::POLLOUT;
        // }
        Ok(status)
    }
}

fn parse(mut reader: UserBufferReader) -> KResult<usize> {
    let mut bytes = alloc::vec![0u8; reader.remaining_len()];
    reader.read_bytes(&mut bytes)?;

    let mut escape_codes = bytes.split(|b| *b == 0x1b);
    if bytes[0] != 0x1b {
        // print until the first escape code
        if let Some(next) = escape_codes.next() {
            fb_print!("{}", core::str::from_utf8(next).unwrap());
        } else {
            return Ok(0);
        }
    }
    for chunk in escape_codes {
        if chunk.is_empty() {
            continue;
        }
        if chunk[0] != b'[' {
            continue;
        }
        let chunk = &chunk[1..];
        // iterate through the chunk until we find one of the ANSI "functions"
        // const ANSI_FUNCTIONS: &[u8] = b"ABCDEFGHJKSTsufm";
        const ANSI_FUNCTIONS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        let res = chunk
            .iter()
            .enumerate()
            .find(|(_i, byte)| ANSI_FUNCTIONS.contains(*byte));
        let (f_idx, function) = if let Some(res) = res {
            res
        } else {
            unreachable!()
        };
        // get its arguments, if any
        let arguments = chunk[..f_idx]
            .split(|byte| *byte == b';')
            .collect::<Vec<&[u8]>>();

        let parse_usize = |arg: &[u8]| core::str::from_utf8(arg).unwrap().parse::<usize>();

        let (x, y) = graphics::cursor_xy();
        match *function {
            b'A' => {
                let n = parse_usize(arguments[0]).unwrap_or(1);
                graphics::set_cursor_y(y.saturating_sub(n));
            }
            b'B' => {
                let n = parse_usize(arguments[0]).unwrap_or(1);
                graphics::set_cursor_y(y.saturating_add(n));
            }
            b'C' => {
                let n = parse_usize(arguments[0]).unwrap_or(1);
                graphics::set_cursor_x(x.saturating_add(n));
            }
            b'D' => {
                let n = parse_usize(arguments[0]).unwrap_or(1);
                graphics::set_cursor_x(x.saturating_sub(n));
            }
            b'E' => {
                let n = parse_usize(arguments[0]).unwrap_or(1);
                graphics::set_cursor_x(0);
                graphics::set_cursor_y(y.saturating_add(n));
            }
            b'F' => {
                let n = parse_usize(arguments[0]).unwrap_or(1);
                graphics::set_cursor_x(0);
                graphics::set_cursor_y(y.saturating_sub(n));
            }
            b'G' | b'f' => {
                let n = parse_usize(arguments[0]).unwrap();
                graphics::set_cursor_x(n);
            }
            b'H' => {
                if arguments[0].is_empty() {
                    graphics::set_cursor_xy((0, 0));
                } else {
                    let n = parse_usize(arguments[0]).unwrap_or(0);
                    let m = parse_usize(arguments[1]).unwrap_or(0);
                    graphics::set_cursor_xy((n, m));
                }
            }
            b'J' => {
                if arguments.is_empty() {
                    graphics::clear_until_end();
                } else {
                    let n = parse_usize(arguments[0]).unwrap_or(0);
                    match n {
                        0 => graphics::clear_until_end(),
                        1 => graphics::clear_until_beginning(),
                        2 => graphics::clear_screen(),
                        3 => todo!("erase saved lines"),
                        _ => unimplemented!(),
                    }
                }
            }
            b'K' => {
                if arguments.is_empty() {
                    graphics::clear_until_eol();
                } else {
                    let n = parse_usize(arguments[0]).unwrap_or(0);
                    match n {
                        0 => graphics::clear_until_eol(),
                        1 => graphics::clear_from_bol(),
                        2 => graphics::clear_line(),
                        _ => unimplemented!(),
                    }
                }
            }
            b'S' => {
                todo!("scroll up by N lines")
            }
            b'T' => {
                todo!("scroll down by N ines")
            }
            b's' => {
                todo!("save cursor position")
            }
            b'u' => {
                todo!("restore cursor postion")
            }
            b'm' => {
                // let arg0 = parse_usize(arguments[0]).unwrap() as u8;
                // match arg0 {
                //     0 => graphics::set_color_code(ColorCode::new(Color::White, Color::Black)),
                //     1 => {} // bold
                //     3 => {} // italic
                //     4 => {} // underline
                //     30..=37 => {
                //         let color = graphics::get_color_code();
                //         graphics::set_color_code(ColorCode::new(
                //             unsafe { core::mem::transmute(arg0 - 30) },
                //             color.background(),
                //         ));
                //     }
                //     40..=47 => {
                //         let color = graphics::get_color_code();
                //         graphics::set_color_code(ColorCode::new(color.foreground(), unsafe {
                //             core::mem::transmute(arg0 - 40)
                //         }));
                //     }
                //     90..=97 => {
                //         todo!("bright foreground color")
                //     }
                //     100..=107 => {
                //         todo!("bright background color")
                //     }
                //     _ => todo!(
                //         "Unknown ANSI function: {}",
                //         core::str::from_utf8(chunk).unwrap()
                //     ),
                // }
            }
            _function if chunk[0] == b'?' => {
                let n = parse_usize(&arguments[0][1..]).unwrap();
                match n {
                    /*
                    Save cursor as in DECSC, xterm.  After
                    saving the cursor, switch to the Alternate Screen Buffer,
                    clearing it first.
                     */
                    1049 => {}
                    n => unimplemented!("Unknown ANSI extension function: {}", n),
                }
            }
            _ => {
                unimplemented!(
                    "Unknown ANSI function: {}",
                    core::str::from_utf8(chunk).unwrap()
                )
            }
        }

        fb_print!("{}", core::str::from_utf8(&chunk[f_idx + 1..]).unwrap());
    }

    Ok(reader.read_len())
}

pub struct PtyMaster {
    wait_queue: WaitQueue,
    buf: IrqMutex<Vec<u8>>,
    discipline: LineDiscipline,
}

impl PtyMaster {
    pub fn new() -> KResult<(Arc<PtyMaster>, Arc<PtySlave>)> {
        let master = Arc::new(PtyMaster {
            wait_queue: WaitQueue::new(),
            buf: IrqMutex::new(Vec::new()),
            discipline: LineDiscipline::new(),
        });
        let slave = Arc::new(PtySlave::new(master.clone()));
        Ok((master, slave))
    }
}

impl FsNode for PtyMaster {
    fn get_name(&self) -> String {
        "tty0".to_owned()
    }
}

impl File for PtyMaster {
    fn read(&self, _offset: usize, buf: UserBufferMut<'_>, options: &OpenFlags) -> KResult<usize> {
        let mut writer = UserBufferWriter::from(buf);
        let timeout = if options.contains(OpenFlags::O_NONBLOCK) {
            Some(0)
        } else {
            None
        };
        let read_len = self.wait_queue.sleep_signalable_until(timeout, || {
            let mut buf_lock = self.buf.lock();
            if buf_lock.is_empty() {
                return Ok(None);
            }

            let copy_len = core::cmp::min(buf_lock.len(), writer.remaining_len());
            writer.write_bytes(&buf_lock[..copy_len])?;
            buf_lock.drain(..copy_len);
            Ok(Some(copy_len))
        })?;

        if read_len > 0 {
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }

        Ok(read_len)
    }

    fn write(&self, _offset: usize, buf: UserBuffer<'_>, _options: &OpenFlags) -> KResult<usize> {
        let written_len = self.discipline.write(buf, |ctrl| {
            let mut master_buf = self.buf.lock();
            match ctrl {
                LineControl::Backspace => {
                    master_buf.extend_from_slice(b"\x08 \x08");
                }
                LineControl::Echo(ch) => {
                    master_buf.push(ch);
                }
            }
        })?;

        if written_len > 0 {
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }

        Ok(written_len)
    }

    fn ioctl(&self, cmd: usize, _arg: usize) -> KResult<isize> {
        log::warn!("ioctl(): unknown cmd for PtyMaster ({:#x})", cmd);
        Ok(0)
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(Stat {
            inode_no: 5,
            mode: FileMode::new(S_IFCHR | 0o666),
            ..Stat::zeroed()
        })
    }

    fn poll(&self) -> KResult<PollStatus> {
        let mut status = PollStatus::empty();
        if !self.buf.lock().is_empty() {
            status |= PollStatus::POLLIN;
        }
        if self.discipline.is_writable() {
            status |= PollStatus::POLLOUT;
        }

        Ok(status)
    }
}

pub struct PtySlave {
    master: Arc<PtyMaster>,
}

impl PtySlave {
    pub fn new(master: Arc<PtyMaster>) -> Self {
        Self { master }
    }
}

impl FsNode for PtySlave {
    fn get_name(&self) -> String {
        "ttyS0".to_owned()
    }
}

impl File for PtySlave {
    fn read(&self, _offset: usize, buf: UserBufferMut, options: &OpenFlags) -> KResult<usize> {
        let read_len = self.master.discipline.read(buf, options)?;
        if read_len > 0 {
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }
        Ok(read_len)
    }

    fn write(&self, _offset: usize, buf: UserBuffer<'_>, _options: &OpenFlags) -> KResult<usize> {
        let mut written_len = 0;
        let mut master_buf = self.master.buf.lock();
        let mut reader = UserBufferReader::from(buf);

        while reader.remaining_len() > 0 {
            let mut tmp = [0; 1];
            let copied_len = reader.read_bytes(&mut tmp)?;
            for ch in &tmp[..copied_len] {
                match *ch {
                    b'\n' => {
                        master_buf.push(b'\r');
                        master_buf.push(b'\n');
                    }
                    _ => {
                        master_buf.push(*ch);
                    }
                }
            }
            written_len += copied_len;
        }

        if written_len > 0 {
            get_scheduler().wake_all(&POLL_WAIT_QUEUE);
        }

        Ok(written_len)
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(Stat {
            inode_no: 6,
            mode: FileMode::new(S_IFCHR | 0o666),
            ..Stat::zeroed()
        })
    }

    fn ioctl(&self, cmd: usize, _arg: usize) -> KResult<isize> {
        const TIOCSPTLCK: usize = 0x40045431;
        match cmd {
            TIOCSPTLCK => Ok(0),
            _ => {
                log::warn!("ioctl(): unknown cmd for PtySlave ({:#x})", cmd);
                Ok(0)
            }
        }
    }

    fn poll(&self) -> KResult<PollStatus> {
        let mut status = PollStatus::empty();

        if self.master.discipline.is_readable() {
            status |= PollStatus::POLLIN;
        }

        status |= PollStatus::POLLOUT;

        Ok(status)
    }
}

pub struct Ptmx {
    pts_dir: Arc<InitRamFsDir>,
}

impl Ptmx {
    pub fn new(pts_dir: Arc<InitRamFsDir>) -> Self {
        Self { pts_dir }
    }
}

impl FsNode for Ptmx {
    fn get_name(&self) -> String {
        todo!()
    }
}

impl File for Ptmx {
    fn open(&self, _options: &OpenFlags) -> KResult<Option<FileRef>> {
        let (master, slave) = PtyMaster::new()?;
        self.pts_dir.add_file(slave);
        Ok(Some(master as FileRef))
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(Stat {
            inode_no: 4,
            mode: FileMode::new(S_IFCHR | 0o666),
            ..Stat::zeroed()
        })
    }

    fn read(&self, _offset: usize, _buf: UserBufferMut, _options: &OpenFlags) -> KResult<usize> {
        unreachable!()
    }

    fn write(&self, _offset: usize, _buf: UserBuffer<'_>, _options: &OpenFlags) -> KResult<usize> {
        unreachable!()
    }

    fn poll(&self) -> KResult<PollStatus> {
        Ok(PollStatus::empty())
    }
}
