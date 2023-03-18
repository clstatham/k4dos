use core::sync::atomic::AtomicUsize;

use alloc::{vec::Vec, collections::BTreeMap};
use x86_64::structures::{paging::PageTableFlags, idt::PageFaultErrorCode};

use crate::mem::{addr::VirtAddr, paging::{units::{MappedPages, PageRange, Page}, mapper::Mapper}, consts::{PAGE_SIZE, USER_VALLOC_BASE}, allocator::{PageAllocator, alloc_kernel_frames}};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VmemAreaId(usize);

#[derive(Clone)]
pub struct VmemArea {
    id: VmemAreaId,
    start_addr: VirtAddr,
    end_addr: VirtAddr,
    flags: PageTableFlags,
}

impl VmemArea {
    pub const fn new(id: VmemAreaId, start_addr: VirtAddr, end_addr: VirtAddr, flags: PageTableFlags) -> Self {
        Self {
            id,
            start_addr,
            end_addr,
            flags,
        }
    }

    pub fn contains_addr(&self, addr: VirtAddr) -> bool {
        (self.start_addr..self.end_addr).contains(&addr)
    }
}

pub struct Vmem {
    areas: BTreeMap<VmemAreaId, VmemArea>,
    mp: BTreeMap<VmemAreaId, Vec<MappedPages>>,
    next_id: AtomicUsize,
    page_allocator: PageAllocator,
}

impl Vmem {
    pub fn new() -> Self {
        let mut page_allocator = PageAllocator::new_vec();
        unsafe {
            page_allocator.insert_free_region(PageRange::new(Page::containing_address(VirtAddr::new(0x400000)), Page::containing_address(VirtAddr::new(usize::MAX).align_down(PAGE_SIZE)) - 1))
        }
        Self { areas: BTreeMap::new(), mp: BTreeMap::new(), next_id: AtomicUsize::new(0), page_allocator }
    }

    fn alloc_id(&self) -> VmemAreaId {
        VmemAreaId(self.next_id.fetch_add(1, core::sync::atomic::Ordering::AcqRel))
    }

    pub fn area_containing(&self, start_addr: VirtAddr, end_addr: VirtAddr) -> Option<VmemAreaId> {
        for (id, area) in self.areas.iter() {
            if area.contains_addr(start_addr) || area.contains_addr(end_addr) {
                return Some(*id)
            }
        }
        None
    }

    pub fn add_area(&mut self, start_addr: VirtAddr, end_addr: VirtAddr, flags: PageTableFlags) {
        assert!(self.area_containing(start_addr, end_addr).is_none(), "Cannot add vmem area as it already exists for these addresses");
        let id = self.alloc_id();
        self.areas.insert(id, VmemArea { id, start_addr, end_addr, flags });
    }

    pub fn handle_page_fault(&mut self, active_mapper: &mut Mapper, faulted_addr: VirtAddr, reason: PageFaultErrorCode) {
        if faulted_addr.align_down(PAGE_SIZE) == VirtAddr::null() {
            todo!("Kill process that accessed null pointer")
        }

        let mut faulted_area = None;
        for (_id, area) in self.areas.iter() {
            if area.contains_addr(faulted_addr) {
                faulted_area = Some(area);
                break;
            }
        }

        if let Some(area) = faulted_area {
            if active_mapper.translate(faulted_addr).is_none() {
                // allocate and map pages
                let page = Page::containing_address(faulted_addr);
                let ap = self.page_allocator.allocate_at(page, 1).unwrap();
                let af = alloc_kernel_frames(1).unwrap();
                let mp = active_mapper.map_to(ap, af, area.flags).unwrap();
                self.mp.get_mut(&area.id).unwrap().push(mp);
            } else {
                // set new flags, handle COW
                let mp = self.mp.get_mut(&area.id).unwrap().iter_mut().find(|mp| mp.pages().contains(Page::containing_address(faulted_addr))).unwrap();
                let orig_flags = mp.flags();
                active_mapper.set_flags(mp, area.flags);

                if area.flags.contains(PageTableFlags::WRITABLE) && !orig_flags.contains(PageTableFlags::WRITABLE) {
                    // COW
                    todo!("COW")
                }
            }
        } else {
            todo!("Kill process that accessed memory it doesn't own")
        }
    }
}
