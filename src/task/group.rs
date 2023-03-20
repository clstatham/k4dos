use alloc::{vec::Vec, sync::{Weak, Arc}};

use crate::util::SpinLock;

use super::{Task, get_scheduler, scheduler::Scheduler, signal::Signal};

pub type PgId = i32;

pub struct TaskGroup {
    pgid: PgId,
    tasks: Vec<Weak<Task>>,
}

impl TaskGroup {
    pub(super) fn new(pgid: PgId) -> Arc<SpinLock<TaskGroup>> {
        let pg = Arc::new(SpinLock::new(TaskGroup { pgid, tasks: Vec::new() }));
        // sched.task_groups.lock().insert(pgid, pg.clone());
        pg
    }

    pub fn pgid(&self) -> PgId {
        self.pgid
    }

    pub fn add(&mut self, task: Weak<Task>) {
        self.tasks.push(task)
    }

    pub fn remove(&mut self, task: &Weak<Task>) {
        self.tasks.retain(|p| !Weak::ptr_eq(p, task));
        if self.tasks.is_empty() {
            get_scheduler().lock().task_groups.lock().remove(&self.pgid);
        }
    }

    pub fn gc_dropped_processes(&mut self) {
        self.tasks.retain(|task| task.upgrade().is_some());
        if self.tasks.is_empty() {
            get_scheduler().lock().task_groups.lock().remove(&self.pgid);
        }
    }

    pub fn signal(&mut self, signal: Signal) {
        for task in self.tasks.iter() {
            // task.upgrade().unwrap().send_signal(signal);
            get_scheduler().lock().send_signal_to(task.upgrade().unwrap(), signal);
        }
    }
}
