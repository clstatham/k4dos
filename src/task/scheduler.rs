use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Weak}, vec::Vec,
};
use atomic_refcell::AtomicRefCell;
use crossbeam_utils::atomic::AtomicCell;
use spin::RwLock;
use x86::current;

use crate::{
    arch::{idt::InterruptFrame, task::arch_context_switch},
    fs::FileRef,
    mem::addr::VirtAddr,
    util::{KResult, SpinLock, ctypes::c_int},
};

use super::{
    get_scheduler,
    group::{PgId, TaskGroup},
    signal::{SigAction, Signal, SIGCHLD},
    Task, TaskId, TaskState, JOIN_WAIT_QUEUE, wait_queue::WaitQueue,
};

pub struct Scheduler {
    run_queue: Arc<SpinLock<VecDeque<Arc<Task>>>>,
    idle_thread: Option<Arc<Task>>,
    preempt_task: Option<Arc<Task>>,
    // reaper_task: Option<Arc<Task>>,
    current_task: Arc<RwLock<Option<Arc<Task>>>>,
    exited_tasks: Arc<SpinLock<Vec<Arc<Task>>>>,
    pub(super) task_groups: SpinLock<BTreeMap<PgId, Arc<SpinLock<TaskGroup>>>>,
}

impl Scheduler {
    pub fn new() -> Arc<Self> {
        let mut s = Self {
            run_queue: Arc::new(SpinLock::new(VecDeque::new())),
            idle_thread: None,
            preempt_task: None,
            // reaper_task: None,
            current_task: Arc::new(RwLock::new(None)),
            task_groups: SpinLock::new(BTreeMap::new()),
            exited_tasks: Arc::new(SpinLock::new(Vec::new())),
        };
        let idle_thread = Task::new_idle(&mut s);
        let preempt_task = Task::new_kernel(&s, preempt, false);
        let reaper_task = Task::new_kernel(&s, reap, true);
        s.idle_thread = Some(idle_thread);
        s.preempt_task = Some(preempt_task);
        // s.reaper_task = Some(reaper_task.clone());
        s.enqueue(reaper_task);
        Arc::new(s)
    }
    

    pub fn enqueue(&self, task: Arc<Task>) {
        task.state.store(TaskState::Runnable);
        self.run_queue.lock().push_back(task)
    }

    pub fn current_task(&self) -> Arc<Task> {
        let current = self.current_task.read();
        let clone = current.as_ref().unwrap().clone();
        drop(current);
        clone
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

    pub fn exit_current(&self, status: c_int) -> ! {
        let current = self.current_task();
        // let current = current.as_ref().unwrap();
        if current.pid.as_usize() == 1 {
            panic!("init (pid=1) tried to exit with status {}", status);
        }

        current.set_state(TaskState::ExitedWith(status));
        if let Some(parent) = current.parent.lock().upgrade() {
            let mut parent_signals = parent.signals.lock();
            if parent_signals.get_action(SIGCHLD) == SigAction::Ignore {
                // parent.children.lock().retain(|p| p.pid != current.pid);
                self.exited_tasks.lock().push(current.clone());
            } else {
                parent_signals.signal(SIGCHLD);
            }
        }

        current.opened_files.lock().close_all();
        self.run_queue.lock().retain(|t| t.pid != current.pid);
        self.wake_all(&JOIN_WAIT_QUEUE.get().unwrap());
        // drop(current);
        self.preempt();
        unreachable!()
    }


    pub fn wake_all(&self, queue: &WaitQueue) {
        let mut q = queue.queue.lock();
        while let Some(proc) = q.pop_front() {
            log::debug!("Waking {}", proc.pid.as_usize());
            self.resume_task(proc)
        }
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
        if self.run_queue.lock().iter().any(|t| t.pid == task.pid) {
            panic!();
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

    pub fn reap_dead(&self) {
        // todo
    }

    pub fn preempt(&self) {
        // unsafe { get_scheduler().force_unlock() };
        let current= self.current_task.read();
        if let Some(current_task) = current.as_ref().cloned() {
            // log::debug!("Switching from PID {:?} to preempt task", current_task.pid);
            // unsafe { self.current_task.force_read_decrement() };
            // core::mem::forget(current);
            drop(current);
            arch_context_switch(current_task.arch_mut(), self.preempt_task.as_ref().unwrap().arch_mut());
        } else {
            // log::debug!("Switching from idle thread to preempt task");
            // if self.current_task.reader_count() > 0 {
            drop(current);
            // unsafe { self.current_task.force_read_decrement() };
            // core::mem::forget(current);
            // }
            arch_context_switch(self.idle_thread.as_ref().unwrap().arch_mut(), self.preempt_task.as_ref().unwrap().arch_mut());
        }
    }
}


pub fn switch() {
    let sched = get_scheduler();
    // let mut sched = sched_lock.lock();
    let mut queue = sched.run_queue.lock();
    if sched.current_task.reader_count() > 0 {
        panic!("{}", sched.current_task.reader_count());
    }
    let mut current = sched.current_task.write();
    let mut task = None;
    loop {
        let t = queue.pop_front();
        if let Some(t) = t {
            if t.get_state() == TaskState::Runnable {
                task = Some(t);
                break;
            }
        } else {
            break;
        }
    }
    if let Some(task) = task {
        if let Some(current_task) = current.as_ref() {
            if current_task.pid != task.pid {
                queue.push_back(current_task.clone());
            }
        }

        core::mem::forget(queue);
        *current = Some(task.clone());
        // log::debug!("Switching from preempt task to PID {:?}", task.pid);
        // unsafe { sched_lock.force_unlock() };
        unsafe { sched.run_queue.force_unlock() };
        drop(current);
        // unsafe { sched.current_task.force_write_unlock() };
        // core::mem::forget(current);
        arch_context_switch(sched.preempt_task.as_ref().unwrap().arch_mut(), task.arch_mut());
    } else {
        if let Some(current_task) = current.as_ref().cloned() {
            let state = { current_task.get_state() };
            if state == TaskState::Runnable {
                // log::debug!("Switching from preempt task to PID {:?} (current task)", current_task.pid);
                unsafe { sched.run_queue.force_unlock() };
                // unsafe { sched_lock.force_unlock() };
                // unsafe { sched.current_task.force_write_unlock() };
                drop(current);
                // core::mem::forget(current);
                arch_context_switch(sched.preempt_task.as_ref().unwrap().arch_mut(), current_task.arch_mut());
                return;
            }
        }

        core::mem::forget(queue);
        *current = None;
        unsafe { sched.run_queue.force_unlock() };
        // unsafe { sched.current_task.force_write_unlock() };
        drop(current);
        // core::mem::forget(current);
        // unsafe { sched_lock.force_unlock() };
        // log::debug!("Switching from preempt task to idle thread");
        // unsafe { sched.current_task.force_write_unlock() };
        // drop(current);
        arch_context_switch(sched.preempt_task.as_ref().unwrap().arch_mut(), sched.idle_thread.as_ref().unwrap().arch_mut());
    }
}

fn reap() {
    let scheduler = get_scheduler();
    loop {
        scheduler.reap_dead();
    }
}

fn preempt() {
    // let scheduler = get_scheduler();
    loop {
        switch();
    }
}
