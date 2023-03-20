use core::{mem::size_of, ops::Add};

use crate::{
    fs::opened_file::FileDesc, mem::addr::VirtAddr, task::current_task,
    userland::buffer::UserBuffer, util::KResult,
};

use super::SyscallHandler;

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
