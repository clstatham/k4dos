use crate::{mem::addr::VirtAddr, task::current_task, util::KResult};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_set_tid_address(&mut self, _addr: VirtAddr) -> KResult<isize> {
        Ok(current_task().pid().as_usize() as isize)
    }

    pub fn sys_getpid(&mut self) -> KResult<isize> {
        Ok(current_task().pid().as_usize() as isize)
    }

    pub fn sys_getppid(&mut self) -> KResult<isize> {
        Ok(current_task().ppid().as_usize() as isize)
    }
}
