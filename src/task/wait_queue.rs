use alloc::{collections::VecDeque, sync::Arc};

use crate::{util::{SpinLock, KResult, errno::Errno}, errno};

use super::{Task, current_task, TaskState, get_scheduler};


pub struct WaitQueue {
    queue: SpinLock<VecDeque<Arc<Task>>>,
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitQueue {
    pub fn new() -> WaitQueue {
        WaitQueue {
            queue: SpinLock::new(VecDeque::new()),
        }
    }

    pub fn sleep_signalable_until<F, R>(&self, mut sleep_if_none: F) -> KResult<R>
    where
        F: FnMut() -> KResult<Option<R>>,
    {
        loop {
            let current = current_task();
            let current = current.as_ref().lock();
            let current = current.as_ref().unwrap().clone();
            let scheduler = get_scheduler();
            current.set_state(TaskState::Waiting);
            self.queue.lock().push_back(current.clone());

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

            if let Some(ret_val) = ret_value {
                scheduler.resume_task(current.clone());
                self.queue
                    .lock()
                    .retain(|proc| !Arc::ptr_eq(proc, &current));
                return ret_val;
            }

            get_scheduler().preempt();
        }
    }

    pub fn wake_all(&self) {
        let mut q = self.queue.lock();
        let sched = get_scheduler();
        while let Some(proc) = q.pop_front() {
            sched.resume_task(proc)
        }
    }
}