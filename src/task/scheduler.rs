use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc},
    vec::Vec,
};
use spin::RwLock;

use crate::{
    arch::{idt::InterruptFrame, task::{arch_context_switch, ArchTask}},
    mem::addr::VirtAddr,
    util::{ctypes::c_int, KResult, IrqMutex, errno::Errno}, errno, task::{JOIN_WAIT_QUEUE},
};

use super::{
    get_scheduler,
    group::{PgId, TaskGroup},
    signal::{SigAction, Signal, SIGCHLD},
    wait_queue::WaitQueue,
    Task, TaskState,
};

pub struct Scheduler {
    run_queue: Arc<IrqMutex<VecDeque<Arc<Task>>>>,
    awaiting_queue: Arc<IrqMutex<VecDeque<Arc<Task>>>>,
    idle_thread: Option<Arc<Task>>,
    preempt_task: Option<Arc<Task>>,
    current_task: Arc<RwLock<Option<Arc<Task>>>>,
    exited_tasks: Arc<IrqMutex<Vec<Arc<Task>>>>,
    pub(super) task_groups: IrqMutex<BTreeMap<PgId, Arc<IrqMutex<TaskGroup>>>>,
}

impl Scheduler {
    pub fn new() -> Arc<Self> {
        let mut s = Self {
            run_queue: Arc::new(IrqMutex::new(VecDeque::new())),
            awaiting_queue: Arc::new(IrqMutex::new(VecDeque::new())),
            idle_thread: None,
            preempt_task: None,
            current_task: Arc::new(RwLock::new(None)),
            task_groups: IrqMutex::new(BTreeMap::new()),
            exited_tasks: Arc::new(IrqMutex::new(Vec::new())),
        };
        let idle_thread = Task::new_idle(&mut s);
        let preempt_task = Task::new_kernel(&s, preempt, false);
        let reaper_task = Task::new_kernel(&s, reap, true);
        s.idle_thread = Some(idle_thread);
        s.preempt_task = Some(preempt_task);
        s.push_runnable(reaper_task);
        Arc::new(s)
    }

    pub fn push_runnable(&self, task: Arc<Task>) {
        task.state.store(TaskState::Runnable);
        let mut queue = self.run_queue.lock();
        self.awaiting_queue.lock().retain(|t| !Arc::ptr_eq(t, &task));
        let already_in_queue = queue.iter().any(|t| Arc::ptr_eq(t, &task));
        if !already_in_queue {
            log::debug!("Pushing {} as runnable", task.pid.as_usize());
            queue.push_back(task);
        }
    }

    pub fn push_awaiting(&self, task: Arc<Task>) {
        task.state.store(TaskState::Waiting);
        let mut queue = self.awaiting_queue.lock();
        self.run_queue.lock().retain(|t| !Arc::ptr_eq(t, &task));
        let already_in_queue = queue.iter().any(|t| Arc::ptr_eq(t, &task));
        if !already_in_queue {
            log::debug!("Pushing {} as waiting", task.pid.as_usize());
            queue.push_back(task);
        }
    }

    pub fn current_task(&self) -> Arc<Task> {
        let current = self.current_task.read();
        let clone = current.as_ref().unwrap().clone();
        drop(current);
        clone
    }

    pub fn find_group(&self, pgid: PgId) -> Option<Arc<IrqMutex<TaskGroup>>> {
        self.task_groups.lock().get(&pgid).cloned()
    }

    pub fn find_or_create_group(&self, pgid: PgId) -> Arc<IrqMutex<TaskGroup>> {
        self.find_group(pgid).unwrap_or_else(|| {
            let g = TaskGroup::new(pgid);
            self.task_groups.lock().insert(pgid, g.clone());
            g
        })
    }

    pub fn exit_current(&self, status: c_int) -> ! {
        let current = self.current_task();
        if current.pid.as_usize() == 1 {
            panic!("init (pid=1) tried to exit with status {}", status);
        }

        current.set_state(TaskState::ExitedWith(status));
        if let Some(parent) = current.parent.lock().upgrade() {
            let mut parent_signals = parent.signals.lock();
            if parent_signals.get_action(SIGCHLD) == SigAction::Ignore {
                parent.children.lock().retain(|p| p.pid != current.pid);
                // self.exited_tasks.lock().push(current.clone());
            } else {
                parent_signals.signal(SIGCHLD);
            }
        }

        current.opened_files.lock().close_all();
        self.run_queue.lock().retain(|t| t.pid != current.pid);
        self.exited_tasks.lock().push(current);
        self.wake_all(&JOIN_WAIT_QUEUE.get().unwrap());
        // drop(current);
        self.preempt();
        unreachable!()
    }

    pub fn wake_all(&self, queue: &WaitQueue) {
        let mut q = queue.queue.lock();
        while let Some(proc) = q.pop_front() {
            self.resume_task(proc)
        }
    }

    pub fn send_signal_to(&self, task: Arc<Task>, signal: Signal) {
        task.signals.lock().signal(signal);
        self.resume_task(task);
    }

    pub fn resume_task(&self, task: Arc<Task>) {
        // let _old_state = task.state.swap(TaskState::Runnable);
        // if old_state == TaskState::Runnable {
        //     return;
        // }
        // if !self.run_queue.lock().iter().any(|t| t.pid == task.pid) {
        self.push_runnable(task);    
        // }
    }

    pub fn try_delivering_signal(&self, frame: &mut InterruptFrame, syscall_result: isize) -> KResult<()> {
        let current = self.current_task();
        if let Some((signal, sigaction)) = current.signals.lock().pop_pending() {
            let mut set = current.sigset.lock();
            if !set.get(signal as usize).as_deref().unwrap_or(&true) {
                match sigaction {
                    SigAction::Ignore => {}
                    // SigAction::Terminate => {
                    //     log::trace!("terminating {:?} by signal {:?}", current.pid, signal);
                    //     self.exit_current(1);
                    // }
                    SigAction::Handler { handler } => {
                        log::trace!("delivering signal {:?} to {:?} (handler addr {:#x})", signal, current.pid, handler as usize);
                        current.signaled_frame.store(Some(frame.clone()));
                        // current.set_signal_mask(SignalMask::Block, set, oldset, length)
                        set.set(signal as usize, true);
                        ArchTask::setup_signal_stack(
                            frame,
                            signal,
                            VirtAddr::new(handler as usize),
                            syscall_result,
                            // VirtAddr::new(sigreturn),
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn restore_signaled_user_stack(&self, current_frame: &mut InterruptFrame) {
        let current = self.current_task();
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
        let current = self.current_task.read();
        if let Some(current_task) = current.as_ref().cloned() {
            // log::debug!("Switching from PID {:?} to preempt task", current_task.pid);
            drop(current);
            arch_context_switch(
                current_task.arch_mut(),
                self.preempt_task.as_ref().unwrap().arch_mut(),
            );
        } else {
            // log::debug!("Switching from idle thread to preempt task");
            drop(current);
            arch_context_switch(
                self.idle_thread.as_ref().unwrap().arch_mut(),
                self.preempt_task.as_ref().unwrap().arch_mut(),
            );
        }
    }

    pub fn sleep(&self, _duration: Option<usize>) -> KResult<()> {
        let current = self.current_task();
        // let awaiting_queue = self.awaiting_queue.lock();

        self.push_awaiting(current);
        self.preempt();

        let current = self.current_task();
        
        if current.has_pending_signals() {
            Err(errno!(Errno::EINTR, "sleep(): pending signals"))
        } else {
            Ok(())
        }
    }
}

pub fn switch() {
    let sched = get_scheduler();
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
            } else {
                queue.push_back(t);
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

        *current = Some(task.clone());
        // log::debug!("Switching from preempt task to PID {:?}", task.pid);
        drop(queue);
        drop(current);
        arch_context_switch(
            sched.preempt_task.as_ref().unwrap().arch_mut(),
            task.arch_mut(),
        );
    } else {
        if let Some(current_task) = current.as_ref().cloned() {
            let state = { current_task.get_state() };
            if state == TaskState::Runnable {
                // log::debug!("Switching from preempt task to PID {:?} (current task)", current_task.pid);
                drop(current);
                drop(queue);
                // core::mem::forget(current);
                arch_context_switch(
                    sched.preempt_task.as_ref().unwrap().arch_mut(),
                    current_task.arch_mut(),
                );
                return;
            }
        }

        *current = None;
        drop(current);
        drop(queue);
        // log::debug!("Switching from preempt task to idle thread");
        arch_context_switch(
            sched.preempt_task.as_ref().unwrap().arch_mut(),
            sched.idle_thread.as_ref().unwrap().arch_mut(),
        );
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
