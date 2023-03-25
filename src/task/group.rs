use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::util::IrqMutex;

use super::{get_scheduler, signal::Signal, Task};

pub type PgId = i32;

pub struct TaskGroup {
    pgid: PgId,
    tasks: Vec<Weak<Task>>,
}

impl TaskGroup {
    pub(super) fn new(pgid: PgId) -> Arc<IrqMutex<TaskGroup>> {
        Arc::new(IrqMutex::new(TaskGroup {
            pgid,
            tasks: Vec::new(),
        }))
    }

    pub fn pgid(&self) -> PgId {
        self.pgid
    }

    pub fn set_pgid(&mut self, pgid: PgId) {
        self.pgid = pgid
    }

    pub fn add(&mut self, task: Weak<Task>) {
        self.tasks.push(task)
    }

    pub fn remove(&mut self, task: &Weak<Task>) {
        self.tasks.retain(|p| !Weak::ptr_eq(p, task));
        if self.tasks.is_empty() {
            get_scheduler().task_groups.lock().remove(&self.pgid);
        }
    }

    pub fn gc_dropped_processes(&mut self) {
        self.tasks.retain(|task| task.upgrade().is_some());
        if self.tasks.is_empty() {
            get_scheduler().task_groups.lock().remove(&self.pgid);
        }
    }

    pub fn signal(&mut self, signal: Signal) {
        for task in self.tasks.iter() {
            get_scheduler().send_signal_to(task.upgrade().unwrap(), signal);
        }
    }
}
