use alloc::{collections::VecDeque, sync::Arc};

use crate::{
    errno,
    util::{errno::Errno, KResult, IrqMutex},
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

    pub fn sleep_signalable_until<F, R>(&self, mut sleep_if_none: F) -> KResult<R>
    where
        F: FnMut() -> KResult<Option<R>>,
    {
        
        loop {
            let current = current_task();
            // let current = current.as_ref().lock();
            // let current = current.as_ref().unwrap().clone();
            let scheduler = get_scheduler();
            current.set_state(TaskState::Waiting);
            {
                let mut q_lock = self.queue.lock();
                if !q_lock.iter().any(|t| Arc::ptr_eq(t, &current)) {
                    q_lock.push_back(current.clone());
                }
            }
            // self.queue.lock().push_back(current.clone());

            if current.has_pending_signals() {
                scheduler.resume_task(current.clone());
                self.queue
                    .lock()
                    .retain(|proc| !Arc::ptr_eq(proc, &current));
                return Err(errno!(Errno::EINTR));
            }

            let ret_value = match sleep_if_none() {
                Ok(Some(ret_val)) => Some(Ok(ret_val)),
                Ok(None) => None,
                Err(err) => Some(Err(err)),
            };

            // let scheduler = get_scheduler();
            if let Some(ret_val) = ret_value {
                scheduler.resume_task(current.clone());
                self.queue
                    .lock()
                    .retain(|proc| !Arc::ptr_eq(proc, &current));
                return ret_val;
            }
            // drop(scheduler);
            // unsafe { get_scheduler().force_unlock() };
            scheduler.sleep(None)?;
            // switch();
        }
    }
}
