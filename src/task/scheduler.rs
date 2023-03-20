use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
};
use atomic_refcell::AtomicRefCell;
use crossbeam_utils::atomic::AtomicCell;
use x86::current;

use crate::{
    arch::{idt::InterruptFrame, task::arch_context_switch},
    fs::FileRef,
    mem::addr::VirtAddr,
    util::{KResult, SpinLock},
};

use super::{
    get_scheduler,
    group::{PgId, TaskGroup},
    signal::{SigAction, Signal},
    Task, TaskId, TaskState,
};

pub struct Scheduler {
    run_queue: Arc<SpinLock<VecDeque<Arc<Task>>>>,
    idle_thread: Option<Arc<Task>>,
    preempt_task: Option<Arc<Task>>,
    current_task: Option<Arc<Task>>,
    pub(super) task_groups: SpinLock<BTreeMap<PgId, Arc<SpinLock<TaskGroup>>>>,
}

impl Scheduler {
    pub fn new() -> Arc<SpinLock<Self>> {
        let mut s = Self {
            run_queue: Arc::new(SpinLock::new(VecDeque::new())),
            idle_thread: None,
            preempt_task: None,
            current_task: None,
            task_groups: SpinLock::new(BTreeMap::new()),
        };
        let idle_thread = Task::new_idle(&mut s);
        let preempt_task = Task::new_kernel(&mut s, preempt, false);
        s.idle_thread = Some(idle_thread);
        s.preempt_task = Some(preempt_task);
        Arc::new(SpinLock::new(s))
    }

    pub fn enqueue(&self, task: Arc<Task>) {
        // todo: set runnable
        self.run_queue.lock().push_back(task)
    }

    pub fn current_task(&self) -> Arc<Task> {
        self.current_task.as_ref().unwrap().clone()
    }

    pub fn find_group(&self, pgid: PgId) -> Option<Arc<SpinLock<TaskGroup>>> {
        self.task_groups.lock().get(&pgid).cloned()
    }

    pub fn find_or_create_group(&self, pgid: PgId) -> Arc<SpinLock<TaskGroup>> {
        self.find_group(pgid).unwrap_or_else(|| {
            let g = TaskGroup::new(pgid);
            self.task_groups.lock().insert(pgid, g.clone());
            g
        })
    }

    pub fn with_kernel_addr_space_active<R>(&self, f: impl FnOnce() -> R) -> R {
        // let mut current = self.current_task.lock();
        // let current = current.as_mut().unwrap();
        // let current = &current.arch_mut().address_space;
        let current = &self.current_task();
        self.idle_thread
            .as_ref()
            .unwrap()
            .arch_mut()
            .address_space
            .switch();
        let res = f();
        current.arch_mut().address_space.switch();
        res
    }

    pub fn exec(&self, file: FileRef, argv: &[&[u8]], envp: &[&[u8]]) {
        // self.current_task.exec(file, argv, envp).unwrap();
        // let mut current = self.current_task.lock();
        todo!()
        // unsafe { self.current_task.force_unlock() };
        // current.as_mut().unwrap().exec(file, argv, envp).unwrap();
    }

    pub fn send_signal_to(&self, task: Arc<Task>, signal: Signal) {
        task.signals.lock().signal(signal);
        self.resume_task(task);
    }

    pub fn resume_task(&self, task: Arc<Task>) {
        let old_state = task.state.swap(TaskState::Runnable);
        if old_state == TaskState::Runnable {
            return;
        }
        self.enqueue(task);
    }

    pub fn try_delivering_signal(&self, frame: &mut InterruptFrame) -> KResult<()> {
        let current = self.current_task();
        if let Some((signal, sigaction)) = current.signals.lock().pop_pending() {
            let set = current.sigset.lock();
            if !set.get(signal as usize).as_deref().unwrap_or(&true) {
                match sigaction {
                    SigAction::Ignore => {}
                    SigAction::Terminate => {
                        log::trace!("terminating {:?} by signal {:?}", current.pid, signal);
                        // self.exit_current(1);
                        todo!("Exit current process");
                    }
                    SigAction::Handler { handler } => {
                        log::trace!("delivering signal {:?} to {:?}", signal, current.pid);
                        current.signaled_frame.store(Some(frame.clone()));
                        // unsafe {
                        current.arch_mut().setup_signal_stack(
                            frame,
                            signal,
                            VirtAddr::new(handler),
                        )?;
                        // }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn restore_signaled_user_stack(current: &Arc<Task>, current_frame: &mut InterruptFrame) {
        if let Some(signaled_frame) = current.signaled_frame.swap(None) {
            current
                .arch_mut()
                .setup_sigreturn_stack(current_frame, &signaled_frame);
        } else {
            log::warn!("User called sigreturn(2) while it is not signaled");
        }
    }

    pub fn preempt(&self) {
        unsafe { get_scheduler().force_unlock() };
        if let Some(current_task) = self.current_task.as_ref() {
            // log::debug!("Switching from PID {:?} to preempt task", current_task.pid);
            arch_context_switch(current_task.arch_mut(), self.preempt_task.as_ref().unwrap().arch_mut());
        } else {
            // log::debug!("Switching from idle thread to preempt task");
            arch_context_switch(self.idle_thread.as_ref().unwrap().arch_mut(), self.preempt_task.as_ref().unwrap().arch_mut());
        }
    }
}


pub fn switch() {
    let sched_lock = get_scheduler();
    let mut sched = sched_lock.lock();
    let mut queue = sched.run_queue.lock();
    if let Some(task) = queue.pop_front() {
        if let Some(current_task) = sched.current_task.as_ref() {
            if current_task.pid != task.pid {
                queue.push_back(current_task.clone());
            }
        }

        core::mem::forget(queue);
        sched.current_task = Some(task.clone());
        // log::debug!("Switching from preempt task to PID {:?}", task.pid);
        unsafe { sched_lock.force_unlock() };
        unsafe { sched.run_queue.force_unlock() };
        arch_context_switch(sched.preempt_task.as_ref().unwrap().arch_mut(), task.arch_mut());
    } else {
        if let Some(current_task) = sched.current_task.as_ref() {
            let state = { current_task.get_state() };
            if state == TaskState::Runnable {
                // log::debug!("Switching from preempt task to PID {:?} (current task)", current_task.pid);
                unsafe { sched.run_queue.force_unlock() };
                unsafe { sched_lock.force_unlock() };
                arch_context_switch(sched.preempt_task.as_ref().unwrap().arch_mut(), current_task.arch_mut());
                return;
            }
        }

        core::mem::forget(queue);
        sched.current_task = None;
        unsafe { sched.run_queue.force_unlock() };
        unsafe { sched_lock.force_unlock() };
        arch_context_switch(sched.preempt_task.as_ref().unwrap().arch_mut(), sched.idle_thread.as_ref().unwrap().arch_mut());
    }
}


fn preempt() {
    // let scheduler = get_scheduler();
    loop {
        switch();
    }
}
