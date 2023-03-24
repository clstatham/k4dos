use alloc::{collections::VecDeque, sync::Arc};

use crate::{
    arch, errno,
    util::{errno::Errno, IrqMutex, KResult},
};

use super::{current_task, get_scheduler, Task, TaskState};

pub struct WaitQueue {
    pub(super) queue: IrqMutex<VecDeque<Arc<Task>>>,
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitQueue {
    pub const fn new() -> WaitQueue {
        WaitQueue {
            queue: IrqMutex::new(VecDeque::new()),
        }
    }

    pub fn sleep_signalable_until<F, R>(
        &self,
        timeout: Option<usize>,
        mut sleep_if_none: F,
    ) -> KResult<R>
    where
        F: FnMut() -> KResult<Option<R>>,
    {
        let start_time = arch::time::get_uptime_ticks();
        loop {
            if let Some(timeout) = timeout {
                if arch::time::get_uptime_ticks() >= start_time + timeout {
                    return Err(errno!(
                        Errno::EINTR,
                        "sleep_signalable_until(): timeout reached"
                    ));
                }
            }
            let current = current_task();
            let scheduler = get_scheduler();
            current.set_state(TaskState::Waiting);
            {
                let mut q_lock = self.queue.lock();
                if !q_lock.iter().any(|t| t.pid == current.pid) {
                    q_lock.push_back(current.clone());
                }
            }

            if current.has_pending_signals() {
                scheduler.resume_task(current.clone());
                self.queue.lock().retain(|t| t.pid != current.pid);
                return Err(errno!(
                    Errno::EINTR,
                    "sleep_signalable_until(): interrupted by pending signals"
                ));
            }

            let ret_value = match sleep_if_none() {
                Ok(Some(ret_val)) => Some(Ok(ret_val)),
                Ok(None) => None,
                Err(err) => Some(Err(err)),
            };

            if let Some(ret_val) = ret_value {
                scheduler.resume_task(current.clone());
                self.queue.lock().retain(|t| t.pid != current.pid);
                return ret_val;
            }
            scheduler.sleep(timeout)?;
        }
    }
}
