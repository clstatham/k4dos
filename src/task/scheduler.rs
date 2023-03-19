use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak},
};
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
    sref: Weak<Self>,
    run_queue: SpinLock<VecDeque<Arc<Task>>>,
    idle_thread: Arc<SpinLock<Option<Arc<Task>>>>,
    preempt_task: Arc<SpinLock<Option<Arc<Task>>>>,
    current_task: Arc<SpinLock<Option<Arc<Task>>>>,
    pub(super) task_groups: SpinLock<BTreeMap<PgId, Arc<SpinLock<TaskGroup>>>>,
}

impl Scheduler {
    pub fn new() -> Arc<Self> {
        let s = Arc::new_cyclic(|sref| Self {
            sref: sref.clone(),
            run_queue: SpinLock::new(VecDeque::new()),
            idle_thread: Arc::new(SpinLock::new(None)),
            preempt_task: Arc::new(SpinLock::new(None)),
            current_task: Arc::new(SpinLock::new(None)),
            task_groups: SpinLock::new(BTreeMap::new()),
        });
        let idle_thread = Task::new_idle(&s);
        let preempt_task = Task::new_kernel(&s, preempt, true);
        *s.idle_thread.lock() = Some(idle_thread);
        *s.preempt_task.lock() = Some(preempt_task);
        s
    }

    pub fn enqueue(&self, task: Arc<Task>) {
        // todo: set runnable
        self.run_queue.lock().push_back(task)
    }

    pub fn current_task(&self) -> Arc<SpinLock<Option<Arc<Task>>>> {
        self.current_task.clone()
    }

    pub fn find_group(&self, pgid: PgId) -> Option<Arc<SpinLock<TaskGroup>>> {
        self.task_groups.lock().get(&pgid).cloned()
    }

    pub fn find_or_create_group(&self, pgid: PgId) -> Arc<SpinLock<TaskGroup>> {
        self.find_group(pgid).unwrap_or_else(|| {
            let g = TaskGroup::new(self.sref.upgrade().unwrap(), pgid);
            self.task_groups.lock().insert(pgid, g.clone());
            g
        })
    }

    pub fn with_kernel_addr_space_active<R>(&self, f: impl FnOnce() -> R) -> R {
        let mut current = self.current_task.lock();
        let current = current.as_mut().unwrap();
        let current = &current.arch_mut().address_space;
        self.preempt_task
            .as_ref()
            .lock()
            .as_ref()
            .unwrap()
            .arch_mut()
            .address_space
            .switch();
        let res = f();
        current.switch();
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
        let current = self.current_task.as_ref().lock();
        let current = current.as_ref().unwrap();

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

    pub fn switch(&self) {
        let mut queue = self.run_queue.lock();
        let mut current_lock = self.current_task.lock();
        let idle = self.idle_thread.as_ref().lock();
        let idle = idle.as_ref().unwrap();
        let preempt = self.preempt_task.as_ref().lock();
        let preempt = preempt.as_ref().unwrap();
        if let Some(task) = queue.pop_front() {
            if let Some(current_task) = current_lock.as_ref() {
                if current_task.pid != task.pid {
                    queue.push_back(current_task.clone());
                }
            }

            *current_lock = Some(task.clone());
            unsafe {
                self.current_task.force_unlock();
                self.run_queue.force_unlock();
                self.idle_thread.force_unlock();
                self.preempt_task.force_unlock();
            }
            log::debug!("Switching from preempt task to PID {:?}", task.pid);
            arch_context_switch(preempt.arch_mut(), task.arch_mut());
        } else {
            if let Some(current_task) = current_lock.as_ref() {
                let state = { current_task.get_state() };
                if state == TaskState::Runnable {
                    unsafe {
                        self.current_task.force_unlock();
                        self.run_queue.force_unlock();
                        self.idle_thread.force_unlock();
                        self.preempt_task.force_unlock();
                    }
                    log::debug!("Switching from preempt task to PID {:?}", current_task.pid);
                    arch_context_switch(preempt.arch_mut(), current_task.arch_mut());
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
        let idle = self.idle_thread.as_ref().lock();
        let idle = idle.as_ref().unwrap();
        let preempt = self.preempt_task.as_ref().lock();
        let preempt = preempt.as_ref().unwrap();
        let current_lock = self.current_task.lock();
        if let Some(current_task) = current_lock.as_ref() {
            unsafe {
                self.current_task.force_unlock();
                self.idle_thread.force_unlock();
                self.preempt_task.force_unlock();
            }
            log::debug!("Switching from PID {:?} to preempt task", current_task.pid);
            arch_context_switch(current_task.arch_mut(), preempt.arch_mut());
        } else {
            unsafe {
                self.current_task.force_unlock();
                self.idle_thread.force_unlock();
                self.preempt_task.force_unlock();
            }
            log::debug!("Switching from idle thread to preempt task");
            arch_context_switch(idle.arch_mut(), preempt.arch_mut());
        }
    }
}

fn preempt() {
    let scheduler = get_scheduler();
    loop {
        scheduler.switch();
    }
}
