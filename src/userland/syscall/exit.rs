use crate::{util::{ctypes::c_int, KResult}, task::{get_scheduler}};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_exit(&mut self, status: c_int) -> KResult<isize> {
        get_scheduler().exit_current(status)
    }
}