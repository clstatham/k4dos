use alloc::sync::Arc;
use paging::units::MemoryUnit;
use spin::Once;
use x86_64::structures::paging::PageTableFlags;

use crate::{
    kerror,
    task::{get_scheduler, Task},
    util::{IrqMutex, KResult},
};

use self::{
    addr_space::AddressSpace,
    allocator::{alloc_kernel_pages_at, GLOBAL_ALLOC},
    consts::{KERNEL_HEAP_SIZE, KERNEL_HEAP_START, PAGE_SIZE},
    paging::{
        mapper::Mapper,
        units::{MappedPages, Page},
    },
};

pub mod addr;
pub mod addr_space;
pub mod allocator;
pub mod consts;
pub mod paging;

pub static KERNEL_ADDR_SPACE: Once<IrqMutex<AddressSpace>> = Once::new();

pub fn remap_kernel() -> KResult<&'static IrqMutex<AddressSpace>> {
    let active = AddressSpace::current();
    log::info!("Active page table at {:?}", active.cr3());
    let new_space = AddressSpace::new()?;

    // and that's all we gotta do, because Offset Page Tables RULE!

    new_space.switch();
    log::info!("Switched to new page table at {:?}", new_space.cr3());
    Ok(KERNEL_ADDR_SPACE.call_once(|| IrqMutex::new(new_space)))
}

pub fn init_heap(kernel_mapper: &mut Mapper) -> KResult<MappedPages> {
    let heap_ap = alloc_kernel_pages_at(
        Page::containing_address(KERNEL_HEAP_START),
        KERNEL_HEAP_SIZE / PAGE_SIZE,
    )?;
    let heap_mp = kernel_mapper.map(
        heap_ap,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
    )?;
    unsafe {
        GLOBAL_ALLOC.lock().add_to_heap(
            heap_mp.pages().start_address().value(),
            heap_mp.pages().end_address().value(),
        );
    }
    Ok(heap_mp)
}

pub fn kernel_addr_space_scope() -> KResult<KernelAddrSpaceGuard> {
    KernelAddrSpaceGuard::new()
}

pub fn with_kernel_addr_space<R, F: FnOnce() -> R>(f: F) -> R {
    let _guard = kernel_addr_space_scope().expect("Failed to switch to kernel address space");
    f()
}

#[must_use = "KernelAddrSpaceGuard restores previous address space on drop"]
pub struct KernelAddrSpaceGuard {
    _private: (),
    current_task: Option<Arc<Task>>,
}

impl KernelAddrSpaceGuard {
    pub fn new() -> KResult<Self> {
        let _kernel_addr_space = KERNEL_ADDR_SPACE
            .get()
            .ok_or(kerror!("KERNEL_ADDR_SPACE not initialized"))?
            .try_lock()?;

        _kernel_addr_space.switch();

        Ok(Self {
            _private: (),
            current_task: get_scheduler().current_task_opt(),
        })
    }
}

impl Drop for KernelAddrSpaceGuard {
    fn drop(&mut self) {
        if let Some(task) = self.current_task.as_ref() {
            task.arch_mut().address_space.switch();
        }
    }
}
