use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{sync::Arc, vec::Vec};
use spin::Once;
use x86_64::structures::idt::PageFaultErrorCode;

use crate::{arch::{task::ArchTask}, mem::addr::VirtAddr, util::SpinLock};

use self::{vmem::Vmem, scheduler::Scheduler};

pub mod scheduler;
pub mod vmem;

static SCHEDULER: Once<Scheduler> = Once::new();

pub fn init() {
    SCHEDULER.call_once(|| Scheduler::new());
}

pub fn get_scheduler() -> &'static Scheduler {
    SCHEDULER.get().unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(usize);

impl TaskId {
    pub const fn new(pid: usize) -> Self {
        Self(pid)
    }

    fn allocate() -> Self {
        static NEXT_PID: AtomicUsize = AtomicUsize::new(1);
        Self::new(NEXT_PID.fetch_add(1, Ordering::AcqRel))
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskState {
    Runnable,
    Waiting,
}

pub struct Task {
    arch: UnsafeCell<ArchTask>,
    state: TaskState,

    pid: TaskId,

    parent: SpinLock<Option<Arc<Task>>>,
    children: SpinLock<Vec<Arc<Task>>>,

    vmem: Arc<SpinLock<Vmem>>,
}

unsafe impl Sync for Task {}

impl Task {
    pub fn new_idle() -> Arc<Task> {
        let pid = TaskId::allocate();
        Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_idle()),
            state: TaskState::Runnable,
            pid,
            parent: SpinLock::new(None),
            children: SpinLock::new(Vec::new()),

            vmem: Arc::new(SpinLock::new(Vmem::new())),
        })
    }

    pub fn new_kernel(entry_point: fn(), enable_interrupts: bool) -> Arc<Task> {
        let pid = TaskId::allocate();
        Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_kernel(
                VirtAddr::new(entry_point as usize),
                enable_interrupts,
            )),
            state: TaskState::Runnable,
            pid,
            parent: SpinLock::new(None),
            children: SpinLock::new(Vec::new()),

            vmem: Arc::new(SpinLock::new(Vmem::new())),
        })
    }

    pub fn arch_mut(&self) -> &mut ArchTask {
        unsafe { &mut *self.arch.get() }
    }

    pub fn handle_page_fault(&self, faulted_addr: VirtAddr, reason: PageFaultErrorCode) {
        let mut addr_space = self.arch_mut().address_space;
        let mut mapper = addr_space.mapper();
        self.vmem.lock().handle_page_fault(&mut mapper, faulted_addr, reason);
    }
}
