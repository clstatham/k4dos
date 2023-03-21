use crate::{
    errno,
    mem::addr::VirtAddr,
    task::{current_task, signal::{SignalMask, SIG_IGN, SigAction, SIG_DFL, DEFAULT_ACTIONS}},
    util::{errno::Errno, error::KResult, ctypes::c_int},
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

    pub fn sys_rt_sigaction(&mut self, signum: c_int, act: VirtAddr, sigreturn: VirtAddr) -> KResult<isize> {
        if act != VirtAddr::null() {
            let handler = *act.read::<usize>()?;
            let sigreturn = *sigreturn.read::<usize>()?;
            let new_action = match handler {
                SIG_IGN => SigAction::Ignore,
                SIG_DFL => match DEFAULT_ACTIONS.get(signum as usize) {
                    Some(def) => *def,
                    None => return Err(errno!(Errno::EINVAL)),
                }
                _ => SigAction::Handler { handler, sigreturn }
            };

            current_task().signals.lock().set_action(signum, new_action)?;
        }
        Ok(0)
    }
}
