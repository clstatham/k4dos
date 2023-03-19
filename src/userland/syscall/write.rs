use crate::{fs::opened_file::FileDesc, mem::addr::VirtAddr, userland::buffer::UserBuffer, util::KResult};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_write(&mut self, fd: FileDesc, addr: VirtAddr, len: usize) -> KResult<isize> {
        // addr.access_ok(len as isize)?;
        let user_buf = UserBuffer::from_vaddr(addr, len);
        // let mut buf = vec![0u8; len];
        // let written_len = user_buf
        //     .read_at(&mut buf, 0, &OpenOptions::empty())
        //     .map_err(|err| errno!(Errno::ETMP))?;
        let file = self.task.as_ref().unwrap().get_opened_file_by_fd(fd)?;
        let written_len = file.write(user_buf)?;
        Ok(written_len as isize)
    }
}