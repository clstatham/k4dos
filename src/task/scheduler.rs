use alloc::{collections::VecDeque, sync::Arc};

use crate::{util::SpinLock, arch::task::arch_context_switch};

use super::{Task, TaskState, get_scheduler};

pub struct Scheduler {
    run_queue: SpinLock<VecDeque<Arc<Task>>>,
    idle_thread: Arc<Task>,
    preempt_task: Arc<Task>,
    current_task: SpinLock<Option<Arc<Task>>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            run_queue: SpinLock::new(VecDeque::new()),
            idle_thread: Task::new_idle(),
            preempt_task: Task::new_kernel(preempt, true),
            current_task: SpinLock::new(None),
        }
    }

    pub fn enqueue(&self, task: Arc<Task>) {
        // todo: set runnable
        self.run_queue.lock().push_back(task)
    }

    pub fn current_task(&self) -> Option<&Arc<Task>> {
        self.current_task.lock().as_ref()
    }

    pub fn switch(&self) {
        let mut queue = self.run_queue.lock();
        let mut current_lock = self.current_task.lock();
        if let Some(task) = queue.pop_front() {
            if let Some(current_task) = current_lock.as_ref() {
                if current_task.pid != task.pid {
                    queue.push_back(current_task.clone());
                }
            }

            *current_lock = Some(task.clone());
            unsafe { self.current_task.force_unlock() };
            unsafe { self.run_queue.force_unlock() };
            arch_context_switch(self.preempt_task.arch_mut(), task.arch_mut());
        } else {
            if let Some(current_task) = current_lock.as_ref() {
                if current_task.state == TaskState::Runnable {
                    unsafe { self.current_task.force_unlock() };
                    unsafe { self.run_queue.force_unlock() };
                    arch_context_switch(self.preempt_task.arch_mut(), current_task.arch_mut());
                    // return;
                }
            }

            // *current_lock = None;
            // unsafe { self.current_task.force_unlock() };
            // unsafe { self.run_queue.force_unlock() };
            // arch_context_switch(self.preempt_task.arch_mut(), self.idle_thread.arch_mut());
        }
    }

    pub fn preempt(&self) {
        // let guard = SavedInterruptStatus::save();
        let current_lock = self.current_task.lock();
        if let Some(current_task) = current_lock.as_ref() {
            unsafe { self.current_task.force_unlock() };
            arch_context_switch(current_task.arch_mut(), self.preempt_task.arch_mut());
        } else {
            unsafe { self.current_task.force_unlock() };
            arch_context_switch(self.idle_thread.arch_mut(), self.preempt_task.arch_mut());
        }
    }
}

fn preempt() {
    let scheduler = get_scheduler();
    loop {
        scheduler.switch();
    }
}
