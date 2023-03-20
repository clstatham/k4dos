use bitflags::bitflags;

use crate::{
    mem::addr::VirtAddr,
    task::{current_task, TaskId, TaskState, JOIN_WAIT_QUEUE},
    util::{ctypes::c_int, KResult},
};

use super::SyscallHandler;

bitflags! {
    pub struct WaitOptions: c_int {
        const WNOHANG   = 1;
        const WUNTRACED = 2;
    }
}

impl<'a> SyscallHandler<'a> {
    pub fn sys_wait4(
        &mut self,
        pid: TaskId,
        status: VirtAddr,
        options: WaitOptions,
        _rusage: VirtAddr, // could be null
    ) -> KResult<isize> {
        let (got_pid, status_val) =
            JOIN_WAIT_QUEUE.get().unwrap().sleep_signalable_until(|| {
                let current = current_task();
                for child in current.children.lock().iter() {
                    if pid.as_usize() > 0 && pid != child.pid() {
                        continue;
                    }

                    if pid.as_usize() == 0 {
                        todo!()
                    }

                    if let TaskState::ExitedWith(status_val) = child.get_state() {
                        return Ok(Some((child.pid(), status_val)));
                    }
                }

                if options.contains(WaitOptions::WNOHANG) {
                    return Ok(Some((TaskId::new(0), 0)));
                }

                Ok(None)
            })?;

        current_task()
            .children
            .lock()
            .retain(|p| p.pid() != got_pid);

        // if let Ok(mut status) = status {
        if status.value() != 0 {
            status.write::<c_int>(status_val)?;
        }

        // }

        Ok(got_pid.as_usize() as isize)
    }
}
