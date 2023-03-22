use core::mem::size_of;

use crate::{mem::addr::VirtAddr, util::{KResult, errno::Errno}, errno, task::current_task, fs::{opened_file::{OpenOptions, FileDesc}, pipe::Pipe}, userland::buffer::UserBufferWriter};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_pipe(&mut self, fds: VirtAddr) -> KResult<isize> {
        if fds == VirtAddr::null() {
            return Err(errno!(Errno::EFAULT))
        }
        // let fds: &mut [FileDesc] = unsafe { core::slice::from_raw_parts_mut::<i32>(fds.as_mut_ptr(), core::mem::size_of::<i32>() * 2) };

        let current = current_task();
        let pipe = current.opened_files.lock().open_pipe(OpenOptions::empty())?;
        
        let write_fd = pipe.write_fd();
        let read_fd = pipe.read_fd();

        // fds[0] = read_fd;
        // fds[1] = write_fd;

        let mut writer = UserBufferWriter::from_vaddr(fds, size_of::<FileDesc>() * 2);
        writer.write(write_fd)?;
        writer.write(read_fd)?;

        Ok(0)
    }
}