use crate::{task::current_task, util::KResult, mem::addr::VirtAddr};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_fork(&mut self) -> KResult<isize> {
        let child = current_task().fork(&self.frame);
        Ok(child.pid().as_usize() as isize)
    }

    pub fn sys_clone(&mut self, _clone_flags: usize, user_stack: VirtAddr, r8:  usize, args: VirtAddr, r9: usize, entry_point: VirtAddr) -> KResult<isize> {
        let child = current_task().clone_process(entry_point, user_stack, args, r8, r9, &self.frame);
        Ok(child.pid().as_usize() as isize)
    }
}
