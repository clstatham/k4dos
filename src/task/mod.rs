use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{sync::{Arc, Weak}, vec::Vec};
use atomic_refcell::AtomicRefCell;
use crossbeam_utils::atomic::AtomicCell;
use spin::Once;
use x86_64::structures::idt::PageFaultErrorCode;

use crate::{
    arch::{task::ArchTask, idt::InterruptFrame},
    fs::{opened_file::{OpenedFileTable, OpenedFile, OpenFlags, OpenOptions, FileDesc}, FileRef, path::{PathComponent, Path}, initramfs::get_root, tty::TTY},
    mem::addr::VirtAddr,
    util::{KResult, SpinLock},
};

use self::{scheduler::Scheduler, vmem::Vmem, signal::{SignalDelivery, SigSet, SignalMask}, group::{TaskGroup, PgId}};

pub mod scheduler;
pub mod vmem;
pub mod wait_queue;
pub mod group;
pub mod signal;

static SCHEDULER: Once<Arc<SpinLock<Scheduler>>> = Once::new();

pub fn init() {
    SCHEDULER.call_once(|| Scheduler::new());
}

pub fn get_scheduler() -> &'static Arc<SpinLock<Scheduler>> {
    SCHEDULER.get().unwrap()
}

pub fn current_task() -> Arc<Task> {
    get_scheduler().lock().current_task().clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(usize);

impl TaskId {
    pub const fn new(pid: usize) -> Self {
        Self(pid)
    }

    fn allocate() -> Self {
        static NEXT_PID: AtomicUsize = AtomicUsize::new(2);
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
    state: AtomicCell<TaskState>,

    pid: TaskId,

    opened_files: Arc<SpinLock<OpenedFileTable>>,

    parent: Weak<Task>,
    children: Arc<SpinLock<Vec<Arc<Task>>>>,
    group: AtomicRefCell<Weak<SpinLock<TaskGroup>>>,

    vmem: Arc<SpinLock<Vmem>>,

    signals: Arc<SpinLock<SignalDelivery>>,
    signaled_frame: AtomicCell<Option<InterruptFrame>>,
    sigset: Arc<SpinLock<SigSet>>,
}

unsafe impl Sync for Task {}

impl Task {
    pub fn new_idle(sched: &mut Scheduler) -> Arc<Task> {
        let pid = TaskId::new(0);
        let group = sched.find_or_create_group(0);
        let t = Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_idle()),
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            parent: Weak::new(),
            children: Arc::new(SpinLock::new(Vec::new())),
            opened_files: Arc::new(SpinLock::new(OpenedFileTable::new())),
            vmem: Arc::new(SpinLock::new(Vmem::new())),
            signaled_frame: AtomicCell::new(None),
            signals: Arc::new(SpinLock::new(SignalDelivery::new())),
            sigset: Arc::new(SpinLock::new(SigSet::ZERO)),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
        });
        group.lock().add(Arc::downgrade(&t));
        t
    }

    pub fn new_kernel(sched: &mut Scheduler, entry_point: fn(), enable_interrupts: bool) -> Arc<Task> {
        let pid = TaskId::allocate();
        let group = sched.find_or_create_group(0);
        let t = Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_kernel(
                VirtAddr::new(entry_point as usize),
                enable_interrupts,
            )),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            parent: Weak::new(),
            children: Arc::new(SpinLock::new(Vec::new())),
            opened_files: Arc::new(SpinLock::new(OpenedFileTable::new())),
            vmem: Arc::new(SpinLock::new(Vmem::new())),
            signaled_frame: AtomicCell::new(None),
            signals: Arc::new(SpinLock::new(SignalDelivery::new())),
            sigset: Arc::new(SpinLock::new(SigSet::ZERO)),
        });
        group.lock().add(Arc::downgrade(&t));
        t
    }

    pub fn new_init(
        file: FileRef,
        sched: &mut Scheduler,
        argv: &[&[u8]],
        envp: &[&[u8]],
    ) -> KResult<Arc<Task>> {
        let pid = TaskId::new(1);

        let console = get_root().unwrap().lookup_path(Path::new("/dev/tty"), true).unwrap();

        let mut files = OpenedFileTable::new();
        // stdin
        files.open_with_fd(0, Arc::new(OpenedFile::new(console.clone(), OpenFlags::O_RDONLY.into(), 0)), OpenOptions::empty())?;
        // stdout
        files.open_with_fd(1, Arc::new(OpenedFile::new(console.clone(), OpenFlags::O_WRONLY.into(), 0)), OpenOptions::empty())?;
        // stderr
        files.open_with_fd(2, Arc::new(OpenedFile::new(console.clone(), OpenFlags::O_WRONLY.into(), 0)), OpenOptions::empty())?;
        let group = sched.find_or_create_group(1);
        let t = Arc::new(Self {
            arch: UnsafeCell::new(ArchTask::new_init(file, argv, envp)?),
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            parent: Weak::new(),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
            children: Arc::new(SpinLock::new(Vec::new())),
            opened_files: Arc::new(SpinLock::new(files)),
            vmem: Arc::new(SpinLock::new(Vmem::new())),
            signaled_frame: AtomicCell::new(None),
            signals: Arc::new(SpinLock::new(SignalDelivery::new())),
            sigset: Arc::new(SpinLock::new(SigSet::ZERO)),
        });
        group.lock().add(Arc::downgrade(&t));
        TTY.get().unwrap().set_foreground_process_group(Arc::downgrade(&group));
        Ok(t)
    }

    #[allow(clippy::mut_from_ref)] // FIXME
    pub fn arch_mut(&self) -> &mut ArchTask {
        unsafe { &mut *self.arch.get() }
    }

    pub fn pid(&self) -> TaskId {
        self.pid
    }

    pub fn get_state(&self) -> TaskState {
        self.state.load()
    }

    pub fn set_state(&self, state: TaskState) {
        self.state.store(state)
    }

    pub fn belongs_to_group(&self, pg: &Weak<SpinLock<TaskGroup>>) -> bool {
        Weak::ptr_eq(&self.group.borrow(), pg)
    }

    pub fn get_opened_file_by_fd(&self, fd: FileDesc) -> KResult<Arc<OpenedFile>> {
        Ok(self.opened_files.lock().get(fd)?.clone())
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


    pub fn set_signal_mask(
        &self,
        how: SignalMask,
        set: VirtAddr,
        oldset: VirtAddr,
        _length: usize,
    ) -> KResult<()> {
        let mut sigset = self.sigset.lock();
        // if let Ok(old) = oldset {
        oldset.write_bytes(sigset.as_raw_slice())?;
        // }

        // if let Ok(new) = set {
            let new_set = set.read::<[u8; 128]>()?;
            let new_set = SigSet::new(*new_set);
            match how {
                SignalMask::Block => *sigset |= new_set,
                SignalMask::Unblock => *sigset &= !new_set,
                SignalMask::Set => *sigset = new_set,
            }
        // }

        Ok(())
    }


    pub fn has_pending_signals(&self) -> bool {
        self.signals.lock().is_pending()
    }
}
