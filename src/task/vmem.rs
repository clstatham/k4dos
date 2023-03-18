use core::sync::atomic::AtomicUsize;

use alloc::{collections::BTreeMap, vec::Vec};
use x86_64::structures::{idt::PageFaultErrorCode, paging::PageTableFlags};

use crate::{mem::{
    addr::VirtAddr,
    allocator::PageAllocator,
    consts::PAGE_SIZE,
    paging::{
        mapper::Mapper,
        units::{MappedPages, Page, PageRange}, table::PagingError,
    },
}, util::KResult, kerr};

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
    pub const fn new(
        id: VmemAreaId,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        flags: PageTableFlags,
    ) -> Self {
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

    pub fn start_address(&self) -> VirtAddr {
        self.start_addr
    }

    pub fn end_address(&self) -> VirtAddr {
        self.end_addr
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
            page_allocator.insert_free_region(PageRange::new(
                Page::containing_address(VirtAddr::new(PAGE_SIZE)),
                Page::containing_address(VirtAddr::new(usize::MAX).align_down(PAGE_SIZE)) - 1,
            ))
        }
        Self {
            areas: BTreeMap::new(),
            mp: BTreeMap::new(),
            next_id: AtomicUsize::new(0),
            page_allocator,
        }
    }

    fn alloc_id(&self) -> VmemAreaId {
        VmemAreaId(
            self.next_id
                .fetch_add(1, core::sync::atomic::Ordering::AcqRel),
        )
    }

    pub fn area_containing(&self, start_addr: VirtAddr, end_addr: VirtAddr) -> Option<VmemAreaId> {
        for (id, area) in self.areas.iter() {
            if area.contains_addr(start_addr) || area.contains_addr(end_addr) {
                return Some(*id);
            }
        }
        None
    }

    pub fn area(&self, id: VmemAreaId) -> Option<&VmemArea> {
        self.areas.get(&id)
    }

    pub fn add_area(&mut self, start_addr: VirtAddr, end_addr: VirtAddr, flags: PageTableFlags) -> VmemAreaId {
        assert!(
            self.area_containing(start_addr, end_addr).is_none(),
            "Cannot add vmem area as it already exists for these addresses"
        );
        let id = self.alloc_id();
        self.areas.insert(
            id,
            VmemArea {
                id,
                start_addr,
                end_addr,
                flags,
            },
        );
        self.mp.insert(id, Vec::new());
        id
    }

    pub fn map_area(&mut self, start_addr: VirtAddr, end_addr: VirtAddr, flags: PageTableFlags, active_mapper: &mut Mapper) -> KResult<VmemAreaId, PagingError> {
        let ap = self.page_allocator.allocate_range(PageRange::new(Page::containing_address(start_addr), Page::containing_address(end_addr - 1))).map_err(|e| kerr!(PagingError::PageAllocationFailed(e)))?;
        let mp = active_mapper.map(ap, flags)?;
        let id = self.add_area(start_addr.align_down(PAGE_SIZE), end_addr.align_up(PAGE_SIZE), flags);
        self.mp.get_mut(&id).unwrap().push(mp);
        Ok(id)
    }

    pub fn unmap_area(&mut self, id: VmemAreaId, active_mapper: &mut Mapper) {
        for mp in self.mp.get_mut(&id).unwrap().iter_mut() {
            active_mapper.unmap(mp);
        }
        self.areas.remove(&id);
        self.mp.remove(&id);
    }

    pub fn clear(&mut self, active_mapper: &mut Mapper) {
        for area_id in self.areas.keys().cloned().collect::<Vec<_>>() {
            self.unmap_area(area_id, active_mapper);
        }
    }

    pub fn handle_page_fault(
        &mut self,
        active_mapper: &mut Mapper,
        faulted_addr: VirtAddr,
        instruction_pointer: VirtAddr,
        reason: PageFaultErrorCode,
    ) {
        log::warn!("User page fault at {:?}!", instruction_pointer);
        log::warn!("Faulted address: {:?}", faulted_addr);
        log::warn!("Reason: {:?}", reason);
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
                // let af = alloc_kernel_frames(1).unwrap();
                let mp = active_mapper.map(ap, area.flags).unwrap();
                self.mp.get_mut(&area.id).unwrap().push(mp);
            } else {
                // set new flags, handle COW
                let mp = self
                    .mp
                    .get_mut(&area.id)
                    .unwrap()
                    .iter_mut()
                    .find(|mp| mp.pages().contains(Page::containing_address(faulted_addr)))
                    .unwrap();
                let orig_flags = mp.flags();
                active_mapper.set_flags(mp, area.flags);

                if area.flags.contains(PageTableFlags::WRITABLE)
                    && !orig_flags.contains(PageTableFlags::WRITABLE)
                {
                    // COW
                    todo!("COW")
                }
            }
        } else {
            todo!("Kill process that accessed memory it doesn't own")
        }
    }
}
