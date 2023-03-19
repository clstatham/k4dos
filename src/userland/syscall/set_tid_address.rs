use crate::{mem::addr::VirtAddr, util::KResult};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_set_tid_address(&mut self, addr: VirtAddr) -> KResult<isize> {
        Ok(self.task.as_ref().unwrap().pid().as_usize() as isize)
    }
}