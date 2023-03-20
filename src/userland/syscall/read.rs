use crate::{
    fs::opened_file::FileDesc, mem::addr::VirtAddr, task::current_task,
    userland::buffer::UserBufferMut, util::KResult,
};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_read(&mut self, fd: FileDesc, vaddr: VirtAddr, len: usize) -> KResult<isize> {
        // vaddr.access_ok(len as isize)?;
        let opened_file = current_task().get_opened_file_by_fd(fd)?;
        let ubuf = UserBufferMut::from_vaddr(vaddr, len);
        let read_len = opened_file.read(ubuf)?;
        Ok(read_len as isize)
    }
}
