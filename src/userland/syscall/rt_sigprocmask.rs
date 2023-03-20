use crate::{
    errno,
    mem::addr::VirtAddr,
    task::{current_task, signal::SignalMask},
    util::{errno::Errno, error::KResult},
};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_rt_sigprocmask(
        &mut self,
        how: usize,
        set: VirtAddr,
        oldset: VirtAddr,
        length: usize,
    ) -> KResult<isize> {
        if length != 8 {
            log::warn!("sys_rt_sigprocmask: length != 8");
        }

        let how = match how {
            0 => SignalMask::Block,
            1 => SignalMask::Unblock,
            2 => SignalMask::Set,
            _ => return Err(errno!(Errno::EINVAL)),
        };

        current_task().set_signal_mask(how, set, oldset, length)?;

        Ok(0)
    }
}
