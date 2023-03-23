use crate::{
    fs::opened_file::FileDesc, mem::addr::VirtAddr, task::current_task,
    userland::buffer::UserBuffer, util::KResult,
};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_write(&mut self, fd: FileDesc, addr: VirtAddr, len: usize) -> KResult<isize> {
        let user_buf = UserBuffer::from_vaddr(addr, len);
        let file = current_task().get_opened_file_by_fd(fd)?;
        let written_len = file.write(user_buf)?;
        Ok(written_len as isize)
    }
}
