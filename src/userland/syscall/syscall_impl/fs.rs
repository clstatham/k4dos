use core::{mem::size_of, ops::Add};

use alloc::{borrow::ToOwned, string::String, sync::Arc};

use crate::{
    bitflags_from_user, errno,
    fs::{
        alloc_inode_no,
        initramfs::file::InitRamFsFile,
        opened_file::{FileDesc, OpenFlags, OpenOptions},
        path::Path,
        FileMode, INode, PollStatus, O_RDWR, O_WRONLY, POLL_WAIT_QUEUE,
    },
    mem::addr::VirtAddr,
    task::current_task,
    userland::{
        buffer::{UserBuffer, UserBufferMut, UserBufferReader, UserBufferWriter},
        syscall::SyscallHandler,
    },
    util::{
        align_up,
        ctypes::{c_int, c_nfds, c_short},
        errno::Errno,
        KResult,
    },
};

pub const F_DUPFD: c_int = 0;
pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;

// Linux-specific commands.
pub const F_LINUX_SPECIFIC_BASE: c_int = 1024;
pub const F_DUPFD_CLOEXEC: c_int = F_LINUX_SPECIFIC_BASE + 6;

impl<'a> SyscallHandler<'a> {
    pub fn sys_fcntl(&mut self, fd: FileDesc, cmd: c_int, arg: usize) -> KResult<isize> {
        let current = current_task();
        let mut files = current.opened_files.lock();
        match cmd {
            F_GETFD => {
                let flags = files.get(fd)?.get_flags();
                Ok(flags.bits() as isize)
            }
            F_SETFD => {
                files.get(fd)?.set_close_on_exec(arg == 1);
                Ok(0)
            }
            F_SETFL => {
                files
                    .get(fd)?
                    .set_flags(OpenFlags::from_bits_truncate(arg as i32))?;
                Ok(0)
            }
            F_DUPFD_CLOEXEC => {
                let fd = files.dup(fd, Some(arg as i32), OpenOptions::new(false, true))?;
                Ok(fd as isize)
            }
            _ => Err(errno!(Errno::ENOSYS, "sys_fctnl(): unknown command")),
        }
    }

    pub fn sys_getcwd(&mut self, buf: VirtAddr, len: usize) -> KResult<isize> {
        let cwd = current_task().root_fs.lock().cwd_path().resolve_abs_path();

        if len < cwd.as_str().as_bytes().len() {
            return Err(errno!(Errno::ERANGE, "sys_getcwd(): buffer too small"));
        }

        let mut cwd = String::from(cwd.as_str());
        cwd.push('\0');
        let buf_val = buf.value();
        let mut writer = UserBufferMut::from_vaddr(buf, len);
        writer
            .write_at(cwd.as_str().as_bytes(), 0, &OpenOptions::empty())
            .unwrap(); // this currently never returns Err; may change
        Ok(buf_val as isize)
    }

    pub fn sys_getdents64(
        &mut self,
        fd: FileDesc,
        dir_ptr: VirtAddr,
        len: usize,
    ) -> KResult<isize> {
        let current = current_task();
        let opened_files = current.opened_files.lock();
        let dir = opened_files.get(fd)?;
        let mut writer = UserBufferWriter::from_vaddr(dir_ptr, len);
        while let Some(entry) = dir.readdir()? {
            let alignment = size_of::<u64>();
            let record_len = align_up(
                size_of::<u64>() * 2 + size_of::<u16>() + 1 + entry.name.len() + 1,
                alignment,
            );
            if writer.written_len() + record_len > len {
                break;
            }

            writer.write(entry.inode_no as u64)?;
            writer.write(dir.pos() as u64)?;
            writer.write(record_len as u16)?;
            writer.write(entry.file_type as u8)?;
            writer.write_bytes(entry.name.as_bytes())?;
            writer.write(0u8)?;
            writer.skip_until_alignment(alignment)?;
        }

        Ok(writer.written_len() as isize)
    }

    pub fn sys_chdir(&mut self, path: &Path) -> KResult<isize> {
        current_task().root_fs.lock().chdir(path)?;
        Ok(0)
    }

    pub fn sys_ioctl(&mut self, fd: FileDesc, cmd: usize, arg: usize) -> KResult<isize> {
        let opened_file = current_task().get_opened_file_by_fd(fd)?;
        opened_file.ioctl(cmd, arg)
    }
}

fn create(path: &Path, flags: OpenFlags, _mode: FileMode) -> KResult<INode> {
    if flags.contains(OpenFlags::O_DIRECTORY) {
        return Err(errno!(Errno::EINVAL, "create(): invalid flags"));
    }

    let (_parent_dir, name) = path
        .parent_and_basename()
        .ok_or_else(|| errno!(Errno::EEXIST, "create(): invalid path"))?;

    let current = current_task();
    let root = current.root_fs.lock();
    let inode = INode::File(Arc::new(InitRamFsFile::new(
        name.to_owned(),
        alloc_inode_no(),
    )));
    root.lookup(path, true)?.as_dir()?.insert(inode.clone());
    Ok(inode)
}

impl<'a> SyscallHandler<'a> {
    pub fn sys_open(&mut self, path: &Path, flags: OpenFlags, mode: FileMode) -> KResult<isize> {
        let current = current_task();
        log::trace!("Attempting to open {}", path);
        if flags.contains(OpenFlags::O_CREAT) {
            match create(path, flags, mode) {
                Ok(_) => {}
                Err(err)
                    if flags.contains(OpenFlags::O_EXCL) && err.errno() == Some(Errno::EEXIST) => {}
                Err(err) => return Err(err),
            }
        }

        let root = current.root_fs.lock();
        let mut opened_files = current.opened_files.lock();
        let path_comp = root.lookup_path(path, true)?;
        if flags.contains(OpenFlags::O_DIRECTORY) && !path_comp.inode.is_dir() {
            return Err(errno!(Errno::ENOTDIR, "sys_open(): not a directory"));
        }
        let access_mode = mode.access_mode();
        if path_comp.inode.is_dir() && (access_mode == O_WRONLY || access_mode == O_RDWR) {
            return Err(errno!(Errno::EISDIR, "sys_open(): is a directory"));
        }

        let fd = opened_files.open(path_comp, flags.into())?;
        log::trace!("Opened {} as {}.", path, fd);
        Ok(fd as isize)
    }

    pub fn sys_close(&mut self, fd: FileDesc) -> KResult<isize> {
        let current = current_task();
        current.opened_files.lock().close(fd)?;
        log::trace!("Closed {}", fd);
        Ok(0)
    }

    pub fn sys_pipe(&mut self, fds: VirtAddr) -> KResult<isize> {
        if fds == VirtAddr::null() {
            return Err(errno!(Errno::EFAULT, "sys_pipe(): null VirtAddr"));
        }
        // let fds: &mut [FileDesc] = unsafe { core::slice::from_raw_parts_mut::<i32>(fds.as_mut_ptr(), core::mem::size_of::<i32>() * 2) };

        let current = current_task();
        let pipe = current
            .opened_files
            .lock()
            .open_pipe(OpenOptions::empty())?;

        let write_fd = pipe.write_fd();
        let read_fd = pipe.read_fd();

        // fds[0] = read_fd;
        // fds[1] = write_fd;

        let mut writer = UserBufferWriter::from_vaddr(fds, size_of::<FileDesc>() * 2);
        writer.write(write_fd)?;
        writer.write(read_fd)?;

        Ok(0)
    }

    pub fn sys_poll(&mut self, fds: VirtAddr, nfds: c_nfds, timeout: c_int) -> KResult<isize> {
        // if timeout > 0 {
        //     log::warn!("Ignoring timeout of {} ms.", timeout);
        // }
        let timeout = if timeout > 0 {
            Some(timeout as usize)
        } else {
            None
        };

        POLL_WAIT_QUEUE.sleep_signalable_until(timeout, || {
            // todo: check timeout

            let mut ready_fds = 0;
            let fds_len = (nfds as usize) * (size_of::<FileDesc>() + 2 * size_of::<c_short>());
            let mut reader = UserBufferReader::from(UserBuffer::from_vaddr(fds, fds_len));
            for _ in 0..nfds {
                let fd = reader.read::<FileDesc>()?;
                // log::debug!("fd: {:?}", fd);
                let events = bitflags_from_user!(PollStatus, reader.read::<c_short>()?);
                // log::debug!("events: {:?}", events);
                if fd < 0 || events.is_empty() {
                    return Err(errno!(Errno::EINVAL));
                } else {
                    let status = current_task().opened_files.lock().get(fd)?.poll()?;
                    // log::debug!("status: {:?}", status);
                    let revents = events & status;
                    if !revents.is_empty() {
                        ready_fds += 1;
                    }
                    // log::debug!("revents: {:?}", revents);

                    fds.add(reader.read_len())
                        .write::<c_short>(revents.bits())?;

                    reader.skip(size_of::<c_short>())?;
                };
            }

            if ready_fds > 0 {
                Ok(Some(ready_fds))
            } else {
                Ok(None)
            }
        })
    }

    pub fn sys_read(&mut self, fd: FileDesc, vaddr: VirtAddr, len: usize) -> KResult<isize> {
        // vaddr.access_ok(len as isize)?;
        let opened_file = current_task().get_opened_file_by_fd(fd)?;
        let ubuf = UserBufferMut::from_vaddr(vaddr, len);
        let read_len = opened_file.read(ubuf)?;
        Ok(read_len as isize)
    }

    pub fn sys_stat(&mut self, path: &Path, buf: VirtAddr) -> KResult<isize> {
        let stat = current_task().root_fs.lock().lookup(path, true)?.stat()?;
        buf.write(stat)?;
        Ok(0)
    }

    pub fn sys_lstat(&mut self, path: &Path, buf: VirtAddr) -> KResult<isize> {
        let stat = current_task().root_fs.lock().lookup(path, false)?.stat()?;
        buf.write(stat)?;
        Ok(0)
    }

    pub fn sys_fstat(&mut self, fd: FileDesc, buf: VirtAddr) -> KResult<isize> {
        let file = current_task().get_opened_file_by_fd(fd)?;
        let stat = file.path().inode.stat()?;
        buf.write(stat)?;
        Ok(0)
    }

    pub fn sys_write(&mut self, fd: FileDesc, addr: VirtAddr, len: usize) -> KResult<isize> {
        let user_buf = UserBuffer::from_vaddr(addr, len);
        let file = current_task().get_opened_file_by_fd(fd)?;
        let written_len = file.write(user_buf)?;
        Ok(written_len as isize)
    }
}

pub const IOV_MAX: usize = 1024;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoVec {
    base: VirtAddr,
    len: usize,
}

impl<'a> SyscallHandler<'a> {
    pub fn sys_writev(
        &mut self,
        fd: FileDesc,
        iov_base: VirtAddr,
        iov_count: usize,
    ) -> KResult<isize> {
        let iov_count = iov_count.min(IOV_MAX);

        let file = current_task().get_opened_file_by_fd(fd)?;
        let mut total: usize = 0;
        for i in 0..iov_count {
            let mut iov: IoVec = *iov_base.add(i * size_of::<IoVec>()).read::<IoVec>()?;

            match total.checked_add(iov.len) {
                Some(len) if len > isize::MAX as usize => {
                    iov.len = isize::MAX as usize - total;
                }
                None => {
                    iov.len = isize::MAX as usize - total;
                }
                _ => {}
            }

            if iov.len == 0 {
                continue;
            }

            total += file.write(UserBuffer::from_vaddr(iov.base, iov.len))?;
        }

        Ok(total as isize)
    }
}
