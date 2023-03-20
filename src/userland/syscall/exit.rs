use crate::{
    task::get_scheduler,
    util::{ctypes::c_int, KResult},
};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_exit(&mut self, status: c_int) -> KResult<isize> {
        get_scheduler().exit_current(status)
    }
}
