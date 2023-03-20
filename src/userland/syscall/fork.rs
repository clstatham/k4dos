use crate::{util::KResult, task::current_task};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_fork(&mut self) -> KResult<isize> {
        let child = current_task().fork(&self.frame);
        Ok(child.pid().as_usize() as isize)
    }
}