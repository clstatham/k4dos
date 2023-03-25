use core::sync::atomic::{AtomicBool, Ordering};

use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec::Vec,
};
use spin::RwLock;
use x86_64::instructions::{hlt, interrupts::enable_and_hlt};

use crate::{
    arch::{
        self,
        idt::InterruptFrame,
        task::{arch_context_switch, ArchTask}, startup_init,
    },
    errno,
    fs::POLL_WAIT_QUEUE,
    mem::addr::VirtAddr,
    task::JOIN_WAIT_QUEUE,
    util::{ctypes::c_int, errno::Errno, IrqMutex, KResult},
};

use super::{
    get_scheduler,
    group::{PgId, TaskGroup},
    signal::{SigAction, Signal, SIGCHLD},
    wait_queue::WaitQueue,
    Task, TaskId, TaskState,
};

pub struct Scheduler {
    tasks: Arc<IrqMutex<BTreeMap<TaskId, Arc<Task>>>>,

    run_queue: Arc<IrqMutex<VecDeque<Arc<Task>>>>,
    waiting_queue: Arc<IrqMutex<VecDeque<Arc<Task>>>>,
    #[allow(clippy::type_complexity)]
    deadline_waiting_queue: Arc<IrqMutex<VecDeque<(Arc<Task>, usize)>>>,

    idle_thread: Option<Arc<Task>>,
    preempt_task: Option<Arc<Task>>,

    current_task: Arc<RwLock<Option<Arc<Task>>>>,
    exited_tasks: Arc<IrqMutex<Vec<Arc<Task>>>>,

    pub(super) task_groups: IrqMutex<BTreeMap<PgId, Arc<IrqMutex<TaskGroup>>>>,
}

impl Scheduler {
    pub fn new() -> Arc<Self> {
        let mut s = Self {
            tasks: Arc::new(IrqMutex::new(BTreeMap::new())),
            run_queue: Arc::new(IrqMutex::new(VecDeque::new())),
            waiting_queue: Arc::new(IrqMutex::new(VecDeque::new())),
            deadline_waiting_queue: Arc::new(IrqMutex::new(VecDeque::new())),
            idle_thread: None,
            preempt_task: None,
            current_task: Arc::new(RwLock::new(None)),
            task_groups: IrqMutex::new(BTreeMap::new()),
            exited_tasks: Arc::new(IrqMutex::new(Vec::new())),
        };
        let idle_thread = Task::new_idle(&mut s);
        let init_task = Task::new_kernel(&s, startup_init, false);
        s.push_runnable(init_task);
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
        self.tasks.lock().try_insert(task.pid, task.clone()).ok();
        self.waiting_queue.lock().retain(|t| t.pid != task.pid);
        let already_in_queue = queue.iter().any(|t| t.pid == task.pid);
        if !already_in_queue {
            // log::debug!("Pushing {} as runnable", task.pid.as_usize());
            queue.push_back(task);
        } else {
            // log::warn!(
            //     "Attempted to push {} as runnable, but it was already runnable",
            //     task.pid.as_usize()
            // );
        }
    }

    pub fn push_waiting(&self, task: Arc<Task>) {
        task.state.store(TaskState::Waiting);
        let mut queue = self.waiting_queue.lock();
        self.tasks.lock().try_insert(task.pid, task.clone()).ok();
        self.run_queue.lock().retain(|t| t.pid != task.pid);
        self.deadline_waiting_queue
            .lock()
            .retain(|(t, _)| t.pid != task.pid);
        let already_in_queue = queue.iter().any(|t| t.pid == task.pid);
        if !already_in_queue {
            // log::debug!("Pushing {} as waiting", task.pid.as_usize());
            queue.push_back(task);
        } else {
            // log::warn!(
            //     "Attempted to push {} as waiting, but it was already waiting",
            //     task.pid.as_usize()
            // );
        }
    }

    pub fn push_deadline_waiting(&self, task: Arc<Task>, duration: usize) {
        task.state.store(TaskState::Waiting);
        let mut queue = self.deadline_waiting_queue.lock();
        self.tasks.lock().try_insert(task.pid, task.clone()).ok();
        self.run_queue.lock().retain(|t| t.pid != task.pid);
        self.waiting_queue.lock().retain(|t| t.pid != task.pid);
        let already_in_queue = queue.iter().any(|(t, _)| t.pid == task.pid);
        if !already_in_queue {
            let deadline = arch::time::get_uptime_ticks() + duration;
            // log::debug!(
            //     "Pushing {} as waiting with duration {} ms",
            //     task.pid.as_usize(),
            //     duration
            // );
            queue.push_back((task, deadline));
        } else {
            // log::warn!(
            //     "Attempted to push {} as waiting with duration {} ms, but it was already waiting",
            //     task.pid.as_usize(),
            //     duration
            // );
        }
    }

    fn check_deadline(&self) {
        let time = arch::time::get_uptime_ticks();
        let mut queue = self.deadline_waiting_queue.lock();
        for _ in 0..queue.len() {
            if let Some((task, deadline)) = queue.pop_front() {
                if deadline <= time {
                    // time's up!
                    // log::debug!(
                    //     "Deadline of {} ms reached for PID {}.",
                    //     deadline,
                    //     task.pid.as_usize()
                    // );
                    drop(queue);
                    self.push_runnable(task);
                    queue = self.deadline_waiting_queue.lock();
                } else {
                    queue.push_back((task, deadline));
                }
            }
        }
    }

    pub fn current_task_opt(&self) -> Option<Arc<Task>> {
        let current = self.current_task.try_read()?;
        let clone = current.as_ref().cloned();
        drop(current);
        clone
    }

    pub fn current_task(&self) -> Arc<Task> {
        let current = self.current_task.read();
        let clone = current.as_ref().unwrap().clone();
        drop(current);
        clone
    }

    pub fn find_task(&self, pid: TaskId) -> Option<Arc<Task>> {
        self.tasks.lock().get(&pid).cloned()
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

    pub fn exit_current(&self, status: c_int) {
        let current = self.current_task();
        if current.pid.as_usize() == 1 {
            panic!("init (pid=1) tried to exit with status {}", status);
        }

        current.set_state(TaskState::ExitedWith(status));
        if let Some(parent) = current.parent.lock().upgrade() {
            let mut parent_signals = parent.signals.lock();
            if parent_signals.get_action(SIGCHLD) == SigAction::Ignore {
                parent.children.lock().retain(|p| p.pid != current.pid);
            } else {
                log::debug!("Sending SIGCHLD to {}", parent.pid.as_usize());
                parent_signals.signal(SIGCHLD);
            }
        }

        current.opened_files.lock().close_all();
        self.run_queue.lock().retain(|t| t.pid != current.pid);
        self.tasks.lock().remove(&current.pid);
        self.exited_tasks.lock().push(current);
        self.wake_all(&JOIN_WAIT_QUEUE);
        self.preempt();
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
        self.push_runnable(task);
    }

    pub fn try_delivering_signal(
        &self,
        frame: &mut InterruptFrame,
        syscall_result: isize,
    ) -> KResult<()> {
        let current = self.current_task();
        if let Some((signal, sigaction)) = current.signals.lock().pop_pending() {
            let mut set = current.sigset.lock();
            if !set.get(signal as usize).as_deref().unwrap_or(&true) {
                match sigaction {
                    SigAction::Ignore => {}
                    SigAction::Terminate => {
                        log::trace!(
                            "terminating pid {} by signal {:?}",
                            current.pid.as_usize(),
                            signal
                        );
                        self.exit_current(1);
                    }
                    SigAction::Handler { handler } => {
                        log::trace!(
                            "delivering signal {:?} to pid {} (handler addr {:#x})",
                            signal,
                            current.pid.as_usize(),
                            handler as usize
                        );
                        current.signaled_frame.store(Some(*frame));
                        set.set(signal as usize, true);
                        ArchTask::setup_signal_stack(
                            frame,
                            signal,
                            VirtAddr::new(handler as usize),
                            syscall_result,
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
        let mut exited = self.exited_tasks.lock();

        for task in exited.iter() {
            self.tasks.lock().remove(&task.pid);
            self.run_queue.lock().retain(|t| t.pid != task.pid);
            self.waiting_queue.lock().retain(|t| t.pid != task.pid);
            JOIN_WAIT_QUEUE.queue.lock().retain(|t| t.pid != task.pid);
            POLL_WAIT_QUEUE.queue.lock().retain(|t| t.pid != task.pid);
            if let Some(group) = task.group.borrow_mut().upgrade() {
                group.lock().gc_dropped_processes();
            }
            // assert_eq!(Arc::strong_count(task), 1, "PID {} has dangling references", task.pid.as_usize());
        }
        exited.clear();
    }

    pub fn preempt(&self) {
        let current = self.current_task.read();
        if let Some(current_task) = current.as_ref().cloned() {
            // log::debug!("Switching from PID {:?} to preempt task", current_task.pid);
            drop(current);
            // unsafe { Arc::decrement_strong_count(Arc::into_raw(current_task))};
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

    pub fn sleep(&self, duration: Option<usize>) -> KResult<()> {
        let current = self.current_task();

        if let Some(duration) = duration {
            self.push_deadline_waiting(current.clone(), duration);
        } else {
            self.push_waiting(current.clone());
        }
        self.preempt();

        if current.has_pending_signals() {
            Err(errno!(Errno::EINTR, "sleep(): pending signals"))
        } else {
            Ok(())
        }
    }
}

pub fn switch() {
    let sched = get_scheduler();
    sched.check_deadline();
    let mut queue = sched.run_queue.lock();
    let mut current = sched
        .current_task
        .try_write()
        .expect("switch(): couldn't lock the current task for switching");
    let mut task = None;
    loop {
        let t = queue.pop_front();
        if let Some(t) = t {
            if t.get_state() == TaskState::Runnable {
                task = Some(t);
                break;
            } else {
                // we'll take the opportunity to purge the run queue of sleeping/dead tasks
                // log::warn!("PID {} was in run queue but not runnable", t.pid.as_usize());
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
        enable_and_hlt();
    }
}

fn preempt() {
    loop {
        switch();
    }
}
