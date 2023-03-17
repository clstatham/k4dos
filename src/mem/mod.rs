use x86_64::{structures::paging::PageTableFlags, registers::control::Cr3};
use xmas_elf::sections::ShType;

use crate::{util::KResult, kerr, backtrace::KERNEL_ELF, mem::{addr::PhysAddr, paging::table::PageTable, allocator::alloc_kernel_frames}, kerrmsg};

use self::{paging::{units::{MappedPages, Page}, table::{PagingError, active_table}, mapper::Mapper}, allocator::{alloc_kernel_pages_at, GLOBAL_ALLOC}, addr::VirtAddr, consts::{KERNEL_HEAP_START, KERNEL_HEAP_SIZE, PAGE_SIZE}};

pub mod addr;
pub mod allocator;
pub mod consts;
pub mod paging;

pub fn remap_kernel() -> KResult<&'static mut PageTable, PagingError> {
    // let kernel_elf = KERNEL_ELF.get().unwrap();
    log::info!("Active page table at {:?}", PhysAddr::new(Cr3::read().0.start_address().as_u64() as usize));
    let active_table = active_table();

    // TODO

    // let frame = alloc_kernel_frames(1).unwrap();
    // let new_table: &mut PageTable = unsafe { &mut *frame.start_address().as_hhdm_virt().as_mut_ptr() };
    // log::info!("Will create new page table at {:?}", frame.start_address());

    // for section in kernel_elf.section_iter() {
    //     log::info!("Section start: {:#x}", section.address());
    //     log::info!("Section size: {:#x}", section.size());
    //     log::info!("");
    // }
    Ok(active_table)
}

pub fn init_heap(kernel_mapper: &mut Mapper) -> KResult<MappedPages, PagingError> {
    let heap_start = VirtAddr::new(KERNEL_HEAP_START);
    let heap_ap = alloc_kernel_pages_at(Page::containing_address(heap_start), KERNEL_HEAP_SIZE / PAGE_SIZE).map_err(|e| kerr!(PagingError::PageAllocationFailed(e)))?;
    let heap_mp = kernel_mapper.map(heap_ap, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE)?;
    unsafe {
        GLOBAL_ALLOC.lock().add_to_heap(heap_mp.pages().start_address().value(), heap_mp.pages().inclusive_end_address().value());
    }
    Ok(heap_mp)
}
