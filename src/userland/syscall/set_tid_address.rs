use crate::{mem::addr::VirtAddr, util::KResult, task::current_task};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_set_tid_address(&mut self, addr: VirtAddr) -> KResult<isize> {
        Ok(current_task().pid().as_usize() as isize)
    }
}