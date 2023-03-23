use crate::{
    errno,
    mem::addr::VirtAddr,
    task::{current_task, signal::{SignalMask, SIG_IGN, SigAction, SIG_DFL, DEFAULT_ACTIONS}, get_scheduler, TaskId},
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
            _ => return Err(errno!(Errno::EINVAL, "sys_rt_sigprocmask(): invalid mask")),
        };

        current_task().set_signal_mask(how, set, oldset, length)?;

        Ok(0)
    }

    pub fn sys_rt_sigaction(&mut self, signum: c_int, act: VirtAddr, oldact: VirtAddr) -> KResult<isize> {
        if oldact != VirtAddr::null() {
            let action = current_task().signals.lock().get_action(signum);
            let action = match action {
                SigAction::Ignore => SIG_IGN,
                SigAction::Terminate => { 0 }, // todo?
                SigAction::Handler { handler } => handler as usize,
            };
            oldact.write(action)?;
        }
        if act != VirtAddr::null() {
            let handler = *act.read::<usize>()?;
            let new_action = match handler {
                SIG_IGN => SigAction::Ignore,
                SIG_DFL => match DEFAULT_ACTIONS.get(signum as usize) {
                    Some(def) => *def,
                    None => return Err(errno!(Errno::EINVAL, "sys_rt_sigaction(): no default action for signal")),
                }
                _ => SigAction::Handler { handler: unsafe { core::mem::transmute(handler) } }
            };

            current_task().signals.lock().set_action(signum, new_action)?;
        }
        
        Ok(0)
    }

    pub fn sys_rt_sigreturn(&mut self) -> KResult<isize> {
        get_scheduler().restore_signaled_user_stack(self.frame);
        Err(errno!(Errno::EINTR, "sys_rt_sigreturn(): interrupted by signal"))
    }

    pub fn sys_kill(&mut self, pid: TaskId, signum: c_int) -> KResult<isize> {
        let sched = get_scheduler();

        sched.send_signal_to(sched.find_task(pid).ok_or(errno!(Errno::ESRCH, "sys_kill(): pid not found"))?, signum);
        Ok(0)
    }
}
