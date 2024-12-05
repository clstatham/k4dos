use crate::{
    kbail, kerror,
    mem::addr::VirtAddr,
    task::{
        current_task, get_scheduler,
        signal::{SigAction, SignalMask, DEFAULT_ACTIONS, SIG_DFL, SIG_ERR, SIG_IGN},
        TaskId,
    },
    userland::syscall::SyscallHandler,
    util::{ctypes::c_int, error::KResult},
};

impl SyscallHandler<'_> {
    pub fn sys_rt_sigprocmask(
        &mut self,
        how: usize,
        set: VirtAddr,
        mut oldset: VirtAddr,
        length: usize,
    ) -> KResult<isize> {
        if length != 8 {
            log::warn!("sys_rt_sigprocmask: length != 8");
        }

        let how = match how {
            0 => SignalMask::Block,
            1 => SignalMask::Unblock,
            2 => SignalMask::Set,
            _ => kbail!(EINVAL, "sys_rt_sigprocmask(): invalid how"),
        };

        current_task().set_signal_mask(how, set, &mut oldset, length)?;

        Ok(0)
    }

    pub fn sys_rt_sigaction(
        &mut self,
        signum: c_int,
        act: VirtAddr,
        oldact: VirtAddr,
    ) -> KResult<isize> {
        if oldact != VirtAddr::null() {
            let action = current_task().signals.lock().get_action(signum);
            let action = match action {
                SigAction::Ignore => SIG_IGN,
                SigAction::Terminate => SIG_ERR, // todo?
                SigAction::Handler { handler } => handler as usize,
            };
            unsafe { oldact.write(action) }?;
        }
        if act != VirtAddr::null() {
            let handler = unsafe { act.read::<usize>() }?;
            let new_action = match handler {
                SIG_IGN => SigAction::Ignore,
                SIG_DFL => match DEFAULT_ACTIONS.get(signum as usize) {
                    Some(def) => *def,
                    None => {
                        kbail!(EINVAL, "sys_rt_sigaction(): invalid signal number");
                    }
                },
                _ => SigAction::Handler {
                    handler: unsafe { core::mem::transmute::<usize, fn()>(handler) },
                },
            };

            current_task()
                .signals
                .lock()
                .set_action(signum, new_action)?;
        }

        Ok(0)
    }

    pub fn sys_rt_sigreturn(&mut self) -> KResult<isize> {
        get_scheduler().restore_signaled_user_stack(self.frame);
        kbail!(EINTR, "sys_rt_sigreturn(): should not return")
    }

    pub fn sys_kill(&mut self, pid: TaskId, signum: c_int) -> KResult<isize> {
        let sched = get_scheduler();

        sched.send_signal_to(
            sched
                .find_task(pid)
                .ok_or(kerror!(ESRCH, "sys_kill(): pid not found"))?,
            signum,
        );
        Ok(0)
    }
}
