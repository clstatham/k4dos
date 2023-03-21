use core::sync::atomic::AtomicUsize;

use alloc::{collections::BTreeMap, vec::Vec};
use x86_64::structures::{idt::PageFaultErrorCode, paging::PageTableFlags};

use crate::{
    arch::idt::InterruptErrorFrame,
    backtrace, errno,
    fs::{opened_file::FileDesc, FileRef},
    mem::{
        addr::VirtAddr,
        allocator::{alloc_kernel_frames, PageAllocator},
        consts::{MAX_LOW_VADDR, PAGE_SIZE, USER_STACK_TOP, USER_VALLOC_BASE, USER_VALLOC_END},
        paging::{
            mapper::Mapper,
            units::{MappedPages, Page, PageRange},
        },
    },
    task::{current_task, get_scheduler, signal::SIGSEGV},
    util::{align_up, errno::Errno, KResult},
};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MMapProt: u64 {
        const PROT_READ = 0x1;
        const PROT_WRITE = 0x2;
        const PROT_EXEC = 0x4;
        const PROT_NONE = 0x0;
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MMapFlags: u64 {
        const MAP_PRIVATE   = 0x02;
        const MAP_FIXED     = 0x10;
        const MAP_ANONYMOUS = 0x20;
    }
}

impl From<MMapProt> for PageTableFlags {
    fn from(e: MMapProt) -> Self {
        let mut res = PageTableFlags::empty();

        res.insert(PageTableFlags::PRESENT);
        res.insert(PageTableFlags::USER_ACCESSIBLE);

        if !e.contains(MMapProt::PROT_EXEC) {
            res.insert(PageTableFlags::NO_EXECUTE);
        }

        if e.contains(MMapProt::PROT_WRITE) {
            res.insert(PageTableFlags::WRITABLE);
        }

        res
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VmemAreaId(usize);

#[derive(Clone)]
pub enum MMapKind {
    Anonymous,
    File {
        file: FileRef,
        offset: usize,
        size: usize,
    },
}

#[derive(Clone)]
pub struct VmemArea {
    id: VmemAreaId,
    start_addr: VirtAddr,
    end_addr: VirtAddr,
    flags: MMapFlags,
    prot: MMapProt,
    kind: MMapKind,
}

impl VmemArea {
    pub const fn new(
        id: VmemAreaId,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        flags: MMapFlags,
        prot: MMapProt,
        kind: MMapKind,
    ) -> Self {
        Self {
            id,
            start_addr,
            end_addr,
            flags,
            prot,
            kind,
        }
    }

    pub fn contains_addr(&self, addr: VirtAddr) -> bool {
        (self.start_addr..self.end_addr).contains(&addr)
    }

    pub fn overlaps_range(&self, start: VirtAddr, end: VirtAddr) -> bool {
        self.contains_addr(start) || self.contains_addr(end)
    }

    pub fn start_address(&self) -> VirtAddr {
        self.start_addr
    }

    pub fn end_address(&self) -> VirtAddr {
        self.end_addr
    }
}

pub struct Vmem {
    areas: Vec<VmemArea>,
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
                Page::containing_address(VirtAddr::new(USER_STACK_TOP) - 1),
            ))
        }
        Self {
            areas: Vec::new(),
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

    pub fn area_containing(
        &mut self,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
    ) -> Option<&mut VmemArea> {
        for area in self.areas.iter_mut() {
            if area.contains_addr(start_addr) && area.contains_addr(end_addr) {
                return Some(area);
            }
        }
        None
    }

    pub fn add_area(
        &mut self,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        flags: MMapFlags,
        prot: MMapProt,
        kind: MMapKind,
    ) -> KResult<VmemAreaId> {
        if self.area_containing(start_addr, end_addr).is_some() {
            // return Err(errno!(Errno::EAGAIN));
            self.log();
            panic!("Cannot add vmem area that already exists");
        }
        let id = self.alloc_id();
        self.areas.push(VmemArea {
            id,
            start_addr,
            end_addr,
            flags,
            prot,
            kind,
        });
        self.areas.sort_by_key(|area| area.start_address());
        self.mp.insert(id, Vec::new());
        Ok(id)
    }

    fn zero_memory(&self, mut start_addr: VirtAddr, end_addr: VirtAddr) -> KResult<()> {
        start_addr.fill(0, end_addr.value() - start_addr.value())?;
        Ok(())
    }

    pub fn mprotect(
        &mut self,
        start_addr: VirtAddr,
        size: usize,
        protection: MMapProt,
    ) -> KResult<()> {
        let area = self
            .area_containing(start_addr, start_addr + size - 1)
            .ok_or(errno!(Errno::ENOMEM))?;
        area.prot = protection;
        Ok(())
    }

    pub fn mmap(
        &mut self,
        start_addr: VirtAddr,
        size: usize,
        protection: MMapProt,
        flags: MMapFlags,
        fd: FileDesc,
        offset: usize,
    ) -> KResult<VirtAddr> {
        if size == 0 {
            return Err(errno!(Errno::EFAULT));
        }

        let size_aligned = align_up(size, PAGE_SIZE);
        if start_addr == VirtAddr::null() {
            let start = self.find_free_space_above(VirtAddr::new(0x7000_0000_0000), size_aligned);
            if let Some((start, prev)) = start {
                if let Some(prev_idx) = prev {
                    let prev = &mut self.areas[prev_idx];
                    if prev.end_addr == start
                        && prev.flags == flags
                        && prev.prot == protection
                        && matches!(prev.kind, MMapKind::Anonymous)
                    {
                        prev.end_addr = start + size_aligned;
                        // self.log();
                        return Ok(start);
                    }
                }

                self.add_area(
                    start,
                    start + size_aligned,
                    flags,
                    protection,
                    MMapKind::Anonymous,
                )?;
                // self.log();
                return Ok(start);
            }

            return Err(errno!(Errno::ENOMEM));
        }
        // todo!()
        Err(errno!(Errno::ENOSYS))
    }

    fn find_free_space_above(
        &mut self,
        minimum_start: VirtAddr,
        size: usize,
    ) -> Option<(VirtAddr, Option<usize>)> {
        if self.areas.is_empty() {
            return Some((minimum_start, None));
        }

        assert!(self.areas.is_sorted_by_key(|a| a.start_addr));
        for i in 0..self.areas.len() - 1 {
            if self.areas[i + 1].start_addr >= minimum_start + size {
                if self.areas[i + 1].start_addr.value() - self.areas[i].end_addr.value() >= size {
                    if self.areas[i].end_addr < minimum_start {
                        return Some((minimum_start, Some(i)));
                    } else {
                        return Some((self.areas[i].end_addr, Some(i)));
                    }
                }
                return None;
            }
        }

        None
    }

    pub fn map_area(
        &mut self,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        flags: MMapFlags,
        prot: MMapProt,
        kind: MMapKind,
        active_mapper: &mut Mapper,
    ) -> KResult<VmemAreaId> {
        let ap = self.page_allocator.allocate_range(PageRange::new(
            Page::containing_address(start_addr),
            Page::containing_address(end_addr - 1),
        ))?;
        let mp = active_mapper.map(ap, prot.into())?;
        let id = self.add_area(
            start_addr.align_down(PAGE_SIZE),
            end_addr.align_up(PAGE_SIZE),
            flags,
            prot,
            kind,
        )?;
        self.mp.get_mut(&id).unwrap().push(mp);
        Ok(id)
    }

    pub fn unmap_area(&mut self, id: VmemAreaId, active_mapper: &mut Mapper) -> Option<()> {
        for mp in self.mp.get_mut(&id)?.iter_mut() {
            active_mapper.unmap(mp);
            unsafe {
                self.page_allocator.insert_free_region(**mp.pages());
            }
        }
        self.areas.retain(|area| area.id != id);
        self.mp.remove(&id);
        Some(())
    }

    pub fn clear(&mut self, active_mapper: &mut Mapper) {
        for id in 0..self.next_id.load(core::sync::atomic::Ordering::Acquire) {
            self.unmap_area(VmemAreaId(id), active_mapper);
        }
    }

    pub fn log(&self) {
        log::debug!("BEGIN VIRTUAL MEMORY STATE DUMP");
        for area in self.areas.iter() {
            log::debug!(
                "{:>16x?} .. {:>16x?}   | {:?}",
                area.start_addr,
                area.end_addr,
                area.flags
            );
        }
        log::debug!("END VIRTUAL MEMORY STATE DUMP");
    }

    pub fn fork_from(&mut self, parent: &Vmem) {
        self.areas = parent.areas.clone();
        self.mp = parent.mp.clone();
        self.page_allocator = parent.page_allocator.clone();
        self.next_id.store(
            parent.next_id.load(core::sync::atomic::Ordering::Acquire),
            core::sync::atomic::Ordering::Release,
        );
    }

    pub fn handle_page_fault(
        &mut self,
        active_mapper: &mut Mapper,
        faulted_addr: VirtAddr,
        stack_frame: InterruptErrorFrame,
        reason: PageFaultErrorCode,
    ) {
        log::warn!("User page fault at {:#x}!", stack_frame.frame.rip as usize);
        log::warn!("PID: {}", current_task().pid().as_usize());
        log::warn!("Faulted address: {:?}", faulted_addr);
        log::warn!("Reason: {:?}", reason);
        // backtrace::unwind_user_stack_from(stack_frame.frame.rbp as usize).unwrap();
        if faulted_addr.align_down(PAGE_SIZE) == VirtAddr::null() {
            // todo!("Kill process that accessed null pointer")
            log::error!("User segmentation fault: null pointer access");
            get_scheduler().send_signal_to(current_task(), SIGSEGV);
            get_scheduler().exit_current(1);
        }

        let mut faulted_area = None;
        for area in self.areas.iter() {
            if area.contains_addr(faulted_addr) {
                faulted_area = Some(area);
                break;
            }
        }

        if let Some(area) = faulted_area {
            // let trans = active_mapper.translate(faulted_addr);
            if !reason.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
                // set present
                // flags |= PageTableFlags::PRESENT;
                // unsafe {
                //     active_mapper.set_flags_single(Page::containing_address(faulted_addr), flags);
                // }
                // if trans.is_none() {
                // allocate and map pages
                let page = Page::containing_address(faulted_addr);
                let ap = self.page_allocator.allocate_at(page, 1).unwrap();
                let mp = active_mapper.map(ap, area.prot.into()).unwrap();
                if !matches!(area.kind, MMapKind::File { .. }) {
                    self.zero_memory(page.start_address(), page.start_address() + PAGE_SIZE)
                    .unwrap();
                }
                self.mp.get_mut(&area.id).unwrap().push(mp);
            } else if reason.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
                if !area.prot.contains(MMapProt::PROT_WRITE) {
                    log::error!("User segmentation fault: illegal write");
                    get_scheduler().send_signal_to(current_task(), SIGSEGV);
                    get_scheduler().exit_current(1);
                }
                // COW
                let new_frame = alloc_kernel_frames(1).unwrap();
                let new_page = unsafe {
                    core::slice::from_raw_parts_mut(
                        new_frame.start_address().as_hhdm_virt().as_mut_ptr::<u8>(),
                        PAGE_SIZE,
                    )
                };
                let old_page = unsafe {
                    core::slice::from_raw_parts(
                        faulted_addr.align_down(PAGE_SIZE).as_ptr::<u8>(),
                        PAGE_SIZE,
                    )
                };
                new_page.copy_from_slice(old_page);
                unsafe {
                    active_mapper.unmap_single(Page::containing_address(faulted_addr));
                }
                active_mapper
                    .map_to_single(
                        Page::containing_address(faulted_addr),
                        new_frame.start(),
                        area.prot.into(),
                    )
                    .unwrap();
            }
        } else {
            todo!("Kill process that accessed memory it doesn't own")
        }
    }
}
