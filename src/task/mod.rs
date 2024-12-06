use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
use atomic_refcell::AtomicRefCell;
use crossbeam_utils::atomic::AtomicCell;
use spin::Once;
use x86_64::structures::idt::PageFaultErrorCode;

use crate::{
    arch::{
        idt::{InterruptErrorFrame, InterruptFrame},
        task::ArchTask,
    },
    fs::{
        initramfs::{get_root, root::RootFs},
        opened_file::{FileDesc, LocalOpenedFile, OpenedFileTable},
        FileRef,
    },
    mem::addr::VirtAddr,
    util::{ctypes::c_int, IrqMutex, KResult},
};

use self::{
    group::{PgId, TaskGroup},
    scheduler::Scheduler,
    signal::{SigSet, SignalDelivery, SignalMask},
    vmem::Vmem,
    wait_queue::WaitQueue,
};

pub mod group;
pub mod scheduler;
pub mod signal;
pub mod vmem;
pub mod wait_queue;

pub static SCHEDULER: Once<Arc<Scheduler>> = Once::new();
pub static JOIN_WAIT_QUEUE: WaitQueue = WaitQueue::new();
pub fn init() {
    SCHEDULER.call_once(Scheduler::new);
}

pub fn get_scheduler() -> &'static Arc<Scheduler> {
    SCHEDULER.get().unwrap()
}

pub fn current_task() -> Arc<Task> {
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
    ExitedWith(c_int),
}

pub struct Task {
    sref: Weak<Task>,

    arch: UnsafeCell<ArchTask>,
    state: AtomicCell<TaskState>,

    pid: TaskId,

    pub(crate) start_time: Once<usize>,

    pub(crate) root_fs: Arc<IrqMutex<RootFs>>,
    pub(crate) opened_files: Arc<IrqMutex<OpenedFileTable>>,

    parent: IrqMutex<Weak<Task>>,
    pub(crate) children: Arc<IrqMutex<Vec<Arc<Task>>>>,
    pub(crate) group: AtomicRefCell<Weak<IrqMutex<TaskGroup>>>,

    vmem: Arc<IrqMutex<Vmem>>,

    pub(crate) signals: Arc<IrqMutex<SignalDelivery>>,
    signaled_frame: AtomicCell<Option<InterruptFrame>>,
    sigset: Arc<IrqMutex<SigSet>>,
}

unsafe impl Sync for Task {}

impl Task {
    pub fn new_idle(sched: &mut Scheduler) -> Arc<Task> {
        let pid = TaskId::new(0);
        let group = sched.find_or_create_group(0);
        let t = Arc::new_cyclic(|sref| Self {
            sref: sref.clone(),
            arch: UnsafeCell::new(ArchTask::new_idle()),
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            start_time: Once::new(),
            parent: IrqMutex::new(Weak::new()),
            children: Arc::new(IrqMutex::new(Vec::new())),
            root_fs: Arc::new(IrqMutex::new(get_root().unwrap().clone())),
            opened_files: Arc::new(IrqMutex::new(OpenedFileTable::new())),
            vmem: Arc::new(IrqMutex::new(Vmem::new())),
            signaled_frame: AtomicCell::new(None),
            signals: Arc::new(IrqMutex::new(SignalDelivery::new())),
            sigset: Arc::new(IrqMutex::new(SigSet::ZERO)),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
        });
        group.lock().add(Arc::downgrade(&t));
        t
    }

    pub fn new_kernel(sched: &Scheduler, entry_point: fn(), enable_interrupts: bool) -> Arc<Task> {
        let pid = TaskId::allocate();
        let group = sched.find_or_create_group(0);
        let t = Arc::new_cyclic(|sref| Self {
            sref: sref.clone(),
            arch: UnsafeCell::new(ArchTask::new_kernel(
                VirtAddr::new(entry_point as usize),
                enable_interrupts,
            )),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            start_time: Once::new(),
            parent: IrqMutex::new(Weak::new()),
            children: Arc::new(IrqMutex::new(Vec::new())),
            root_fs: Arc::new(IrqMutex::new(get_root().unwrap().clone())),
            opened_files: Arc::new(IrqMutex::new(OpenedFileTable::new())),
            vmem: Arc::new(IrqMutex::new(Vmem::new())),
            signaled_frame: AtomicCell::new(None),
            signals: Arc::new(IrqMutex::new(SignalDelivery::new())),
            sigset: Arc::new(IrqMutex::new(SigSet::ZERO)),
        });
        group.lock().add(Arc::downgrade(&t));
        t
    }

    pub fn exec(&self, file: FileRef, argv: &[&[u8]], envp: &[&[u8]]) -> KResult<()> {
        {
            self.opened_files.lock().close_cloexec_files();
            self.arch_mut().address_space.with_mapper(|mut mapper| {
                self.vmem.lock().clear(&mut mapper);
            });
            *self.signals.lock() = SignalDelivery::new();
            *self.sigset.lock() = SigSet::ZERO;
            self.signaled_frame.store(None);
        }
        let lock = &mut self.vmem.lock();
        unsafe { self.vmem.force_unlock() };
        self.arch_mut().exec(lock, file, argv, envp)
    }

    pub fn make_child(&self, arch: UnsafeCell<ArchTask>) -> Arc<Task> {
        let pid = TaskId::allocate();

        let group = self.group.borrow().upgrade().unwrap();
        let new = Arc::new_cyclic(|sref| Self {
            sref: sref.clone(),
            arch,
            root_fs: Arc::new(IrqMutex::new(self.root_fs.lock().clone())),
            opened_files: Arc::new(IrqMutex::new(self.opened_files.lock().clone())), // todo: deeper clone
            children: Arc::new(IrqMutex::new(Vec::new())),
            parent: IrqMutex::new(Weak::new()),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            start_time: Once::new(),
            vmem: Arc::new(IrqMutex::new(Vmem::new())),
            signals: Arc::new(IrqMutex::new(SignalDelivery::new())),
            signaled_frame: AtomicCell::new(None),
            sigset: Arc::new(IrqMutex::new(SigSet::ZERO)),
        });
        self.add_child(new.clone());
        new.signals.lock().clone_from(&self.signals.lock());
        new.vmem.lock().fork_from(&self.vmem.lock());
        group.lock().add(Arc::downgrade(&new));
        get_scheduler().push_runnable(new.clone(), false);
        new
    }

    pub fn fork(&self) -> Arc<Task> {
        let arch = UnsafeCell::new(self.arch_mut().fork().unwrap());
        self.make_child(arch)
    }

    pub fn clone_process(
        &self,
        entry_point: VirtAddr,
        user_stack: VirtAddr,
        args: VirtAddr,
        r8: usize,
        r9: usize,
        syscall_frame: &InterruptFrame,
    ) -> Arc<Task> {
        let arch = UnsafeCell::new(
            self.arch_mut()
                .clone_process(entry_point, user_stack, args, r8, r9, syscall_frame)
                .unwrap(),
        );
        let pid = TaskId::allocate();

        let group = self.group.borrow().upgrade().unwrap();
        let t = Arc::new_cyclic(|sref| Self {
            sref: sref.clone(),
            arch,
            opened_files: Arc::new(IrqMutex::new(self.opened_files.lock().clone())), // todo: deeper clone
            state: AtomicCell::new(TaskState::Runnable),
            pid,
            start_time: Once::new(),
            root_fs: Arc::new(IrqMutex::new(self.root_fs.lock().clone())), // todo: actually fork the root fs
            children: Arc::new(IrqMutex::new(Vec::new())),
            parent: IrqMutex::new(Weak::new()),
            group: AtomicRefCell::new(Arc::downgrade(&group)),
            signals: Arc::new(IrqMutex::new(self.signals.lock().clone())),
            signaled_frame: AtomicCell::new(None),
            sigset: Arc::new(IrqMutex::new(SigSet::ZERO)),
            vmem: self.vmem.clone(), // important: we don't fork_from here
        });
        self.add_child(t.clone());
        group.lock().add(Arc::downgrade(&t));
        get_scheduler().push_runnable(t.clone(), false);
        t
    }

    fn add_child(&self, child: Arc<Task>) {
        let mut children = self.children.lock();
        child.set_parent(self.sref.clone());
        children.push(child);
    }

    #[allow(clippy::mut_from_ref)] // FIXME
    pub fn arch_mut(&self) -> &mut ArchTask {
        unsafe { &mut *self.arch.get() }
    }

    pub fn pid(&self) -> TaskId {
        self.pid
    }

    pub fn ppid(&self) -> TaskId {
        if let Some(parent) = self.parent.lock().upgrade() {
            parent.pid
        } else {
            TaskId::new(0)
        }
    }

    pub fn pgid(&self) -> Option<PgId> {
        Some(self.group.borrow().upgrade()?.lock().pgid())
    }

    pub fn get_state(&self) -> TaskState {
        self.state.load()
    }

    pub fn set_state(&self, state: TaskState) {
        if !matches!(self.get_state(), TaskState::ExitedWith(_)) {
            self.state.store(state)
        } else {
            unreachable!();
        }
    }

    fn set_parent(&self, parent: Weak<Task>) {
        *self.parent.lock() = parent;
    }

    pub fn belongs_to_group(&self, pg: &Weak<IrqMutex<TaskGroup>>) -> bool {
        Weak::ptr_eq(&self.group.borrow(), pg)
    }

    pub fn get_opened_file_by_fd(&self, fd: FileDesc) -> KResult<LocalOpenedFile> {
        Ok(self.opened_files.lock().get(fd)?.clone())
    }

    pub fn vmem(&self) -> Arc<IrqMutex<Vmem>> {
        self.vmem.clone()
    }

    pub fn handle_page_fault(
        &self,
        faulted_addr: VirtAddr,
        stack_frame: InterruptErrorFrame,
        reason: PageFaultErrorCode,
    ) -> KResult<()> {
        let addr_space = &mut self.arch_mut().address_space;
        self.vmem
            .try_lock()?
            .handle_page_fault(addr_space, faulted_addr, stack_frame, reason)
    }

    pub fn set_signal_mask(
        &self,
        how: SignalMask,
        set: VirtAddr,
        oldset: &mut VirtAddr,
        _length: usize,
    ) -> KResult<()> {
        let mut sigset = self.sigset.lock();
        if !oldset.is_null() {
            let slice = sigset.as_raw_slice();
            assert_eq!(slice.len(), 8);
            unsafe { oldset.write_bytes_user(slice) }?;
        }

        if !set.is_null() {
            let new_set = unsafe { set.read_user::<[u8; 8]>()? };
            let new_set = SigSet::new(new_set);
            match how {
                SignalMask::Block => *sigset |= new_set,
                SignalMask::Unblock => *sigset &= !new_set,
                SignalMask::Set => *sigset = new_set,
            }
        }

        Ok(())
    }

    pub fn has_pending_signals(&self) -> bool {
        self.signals.lock().is_pending()
    }
}
