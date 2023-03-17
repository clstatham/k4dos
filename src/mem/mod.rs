use x86_64::structures::paging::PageTableFlags;

use crate::{kerr, util::KResult};

use self::{
    addr::VirtAddr,
    addr_space::AddressSpace,
    allocator::{alloc_kernel_pages_at, GLOBAL_ALLOC},
    consts::{KERNEL_HEAP_SIZE, KERNEL_HEAP_START, PAGE_SIZE},
    paging::{
        mapper::Mapper,
        table::PagingError,
        units::{MappedPages, Page},
    },
};

pub mod addr;
pub mod addr_space;
pub mod allocator;
pub mod consts;
pub mod paging;

pub fn remap_kernel() -> KResult<AddressSpace, PagingError> {
    let active = AddressSpace::current();
    log::info!("Active page table at {:?}", active.cr3());

    let new_space = AddressSpace::new()?;

    // and that's all we gotta do, because Limine and Offset Page Tables RULE!

    new_space.switch();
    log::info!("Switched to new page table at {:?}", new_space.cr3());
    Ok(new_space)
}

pub fn init_heap(kernel_mapper: &mut Mapper) -> KResult<MappedPages, PagingError> {
    let heap_start = VirtAddr::new(KERNEL_HEAP_START);
    let heap_ap = alloc_kernel_pages_at(
        Page::containing_address(heap_start),
        KERNEL_HEAP_SIZE / PAGE_SIZE,
    )
    .map_err(|e| kerr!(PagingError::PageAllocationFailed(e)))?;
    let heap_mp = kernel_mapper.map(
        heap_ap,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
    )?;
    unsafe {
        GLOBAL_ALLOC.lock().add_to_heap(
            heap_mp.pages().start_address().value(),
            heap_mp.pages().inclusive_end_address().value(),
        );
    }
    Ok(heap_mp)
}
