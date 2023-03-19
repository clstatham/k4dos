use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{sync::Arc, vec::Vec};
use spin::Once;
use x86_64::structures::idt::PageFaultErrorCode;

use crate::{
    arch::task::ArchTask,
    fs::{opened_file::OpenedFileTable, FileRef},
    mem::addr::VirtAddr,
    userland::elf::ElfLoadError,
    util::{KResult, SpinLock},
};

use self::{scheduler::Scheduler, vmem::Vmem};

pub mod scheduler;
pub mod vmem;

static SCHEDULER: Once<Scheduler> = Once::new();

pub fn init() {
    SCHEDULER.call_once(|| Scheduler::new());
}

pub fn get_scheduler() -> &'static Scheduler {
    SCHEDULER.get().unwrap()
}

pub fn current_task() -> Arc<SpinLock<Option<Arc<Task>>>> {
    get_scheduler().current_task()
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
    state: SpinLock<TaskState>,

    pid: TaskId,

    opened_files: Arc<SpinLock<OpenedFileTable>>,

    parent: Arc<SpinLock<Option<Arc<Task>>>>,
    children: Arc<SpinLock<Vec<Arc<Task>>>>,

    vmem: Arc<SpinLock<Vmem>>,
}

unsafe impl Sync for Task {}

impl Task {
    pub fn new_idle() -> Arc<Task> {
        let pid = TaskId::allocate();
        Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_idle()),
            state: SpinLock::new(TaskState::Runnable),
            pid,
            parent: Arc::new(SpinLock::new(None)),
            children: Arc::new(SpinLock::new(Vec::new())),
            opened_files: Arc::new(SpinLock::new(OpenedFileTable::new())),
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
            state: SpinLock::new(TaskState::Runnable),
            pid,
            parent: Arc::new(SpinLock::new(None)),
            children: Arc::new(SpinLock::new(Vec::new())),
            opened_files: Arc::new(SpinLock::new(OpenedFileTable::new())),
            vmem: Arc::new(SpinLock::new(Vmem::new())),
        })
    }

    pub fn arch_mut(&self) -> &mut ArchTask {
        unsafe { &mut *self.arch.get() }
    }

    pub fn pid(&self) -> TaskId {
        self.pid
    }

    pub fn handle_page_fault(
        &self,
        faulted_addr: VirtAddr,
        instruction_pointer: VirtAddr,
        reason: PageFaultErrorCode,
    ) {
        let addr_space = &mut self.arch_mut().address_space;
        let mut mapper = addr_space.mapper();
        self.vmem
            .lock()
            .handle_page_fault(&mut mapper, faulted_addr, instruction_pointer, reason);
    }

    pub fn new_init(
        file: FileRef,
        argv: &[&[u8]],
        envp: &[&[u8]],
    ) -> KResult<Arc<Task>, ElfLoadError> {
        // {
        //     self.opened_files.lock().close_cloexec_files();
        // }

        // let mut vmem = self.vmem.lock();
        // vmem.clear(&mut self.arch_mut().address_space.mapper());

        // unsafe { self.vmem.force_unlock() };
        // self.arch_mut().exec(file, argv, envp).unwrap();

        let pid = TaskId::allocate();
        Ok(Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_init(file, argv, envp)?),
            state: SpinLock::new(TaskState::Runnable),
            pid,
            parent: Arc::new(SpinLock::new(None)),
            children: Arc::new(SpinLock::new(Vec::new())),
            opened_files: Arc::new(SpinLock::new(OpenedFileTable::new())),
            vmem: Arc::new(SpinLock::new(Vmem::new())),
        }))
    }
}
