use core::sync::atomic::AtomicUsize;

use alloc::vec::Vec;
use x86::controlregs::cr3;
use x86_64::structures::{idt::PageFaultErrorCode, paging::PageTableFlags};

use crate::{
    arch::idt::InterruptErrorFrame,
    backtrace,
    fs::{
        opened_file::{FileDesc, OpenFlags},
        FileRef,
    },
    kbail, kerror,
    mem::{
        addr::VirtAddr,
        addr_space::AddressSpace,
        allocator::{alloc_kernel_frames, free_kernel_frames, PageAllocator},
        consts::{PAGE_SIZE, USER_STACK_TOP, USER_VALLOC_BASE},
        paging::{
            mapper::Mapper,
            units::{AllocatedFrames, Frame, FrameRange, MemoryUnit, Page, PageRange},
        },
    },
    task::{current_task, get_scheduler, signal::SIGSEGV},
    userland::buffer::UserBufferMut,
    util::{align_up, KResult},
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
    start_addr: VirtAddr,
    end_addr: VirtAddr,
    flags: MMapFlags,
    pub(crate) prot: MMapProt,
    kind: MMapKind,
}

impl VmemArea {
    pub const fn new(
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        flags: MMapFlags,
        prot: MMapProt,
        kind: MMapKind,
    ) -> Self {
        Self {
            start_addr,
            end_addr,
            flags,
            prot,
            kind,
        }
    }

    pub const fn null() -> Self {
        Self {
            start_addr: VirtAddr::null(),
            end_addr: VirtAddr::null(),
            flags: MMapFlags::empty(),
            prot: MMapProt::PROT_NONE,
            kind: MMapKind::Anonymous,
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

    pub fn size_in_bytes(&self) -> usize {
        self.end_addr.value() - self.start_addr.value()
    }

    pub fn merge_with(&mut self, other: Self) -> KResult<()> {
        if other.size_in_bytes() == 0 {
            return Ok(());
        }
        if other.start_addr != self.end_addr && other.end_addr != self.start_addr {
            kbail!("Cannot merge non-contiguous areas");
        }
        if other.start_addr < self.start_addr {
            self.start_addr = other.start_addr;
        }
        if other.end_addr > self.end_addr {
            self.end_addr = other.end_addr;
        }

        Ok(())
    }
}

pub struct Vmem {
    areas: Vec<VmemArea>,
    next_id: AtomicUsize,
    page_allocator: PageAllocator,
}

impl Vmem {
    pub fn new() -> Self {
        let mut page_allocator = PageAllocator::new_vec();
        unsafe {
            page_allocator.insert_free_region(PageRange::new(
                Page::containing_address(VirtAddr::new(PAGE_SIZE)),
                Page::containing_address(USER_STACK_TOP),
            ))
        }
        Self {
            areas: Vec::new(),
            next_id: AtomicUsize::new(0),
            page_allocator,
        }
    }

    pub fn area_containing_mut(
        &mut self,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
    ) -> Option<&mut VmemArea> {
        self.areas
            .iter_mut()
            .find(|area| area.contains_addr(start_addr) && area.contains_addr(end_addr))
    }

    pub fn area_containing(&self, start_addr: VirtAddr, end_addr: VirtAddr) -> Option<&VmemArea> {
        self.areas
            .iter()
            .find(|area| area.contains_addr(start_addr) && area.contains_addr(end_addr))
    }

    pub fn add_area(
        &mut self,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        flags: MMapFlags,
        prot: MMapProt,
        kind: MMapKind,
    ) -> KResult<()> {
        if self.area_containing_mut(start_addr, end_addr).is_some() {
            self.log();
            panic!("Cannot add vmem area that already exists");
        }
        self.areas.push(VmemArea {
            start_addr,
            end_addr,
            flags,
            prot,
            kind,
        });
        self.areas.retain(|a| a.start_addr != a.end_addr);
        self.areas.sort_by_key(|area| area.start_address().value());
        self.merge_contiguous_chunks();
        Ok(())
    }

    pub fn merge_contiguous_chunks(&mut self) {
        let mut i = 0;
        while i < self.areas.len() - 1 {
            let mut j = i + 1;
            while j < self.areas.len() {
                if self.areas[i]
                    .overlaps_range(self.areas[j].start_address(), self.areas[j].end_address())
                {
                    let old = self.areas.remove(j);
                    self.areas[i].merge_with(old).expect("Error merging pages");
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
    }

    fn zero_memory(&self, start_addr: VirtAddr, end_addr: VirtAddr) -> KResult<()> {
        unsafe { start_addr.fill(0, end_addr.value() - start_addr.value()) }?;
        Ok(())
    }

    pub fn mprotect(
        &mut self,
        start_addr: VirtAddr,
        size: usize,
        protection: MMapProt,
    ) -> KResult<()> {
        let area = self
            .area_containing_mut(start_addr, start_addr + size - 1)
            .ok_or(kerror!(ENOMEM, "mprotect(): no areas containing address"))?;
        area.prot = protection;
        Ok(())
    }

    pub fn mremap(
        &mut self,
        old_addr: VirtAddr,
        old_size: usize,
        new_size: usize,
        active_mapper: &mut Mapper,
    ) -> KResult<VirtAddr> {
        if new_size == 0 {
            return Err(kerror!(EINVAL, "mremap(): new_size is zero"));
        }

        // let new_size_aligned = align_up(new_size, PAGE_SIZE);
        let conflicting_area = self
            .area_containing(old_addr + new_size, old_addr + new_size)
            .cloned();
        let old_area = self.area_containing_mut(old_addr, old_addr + old_size);

        if let Some(old_area) = old_area {
            if let Some(ref conflicting_area) = conflicting_area {
                if conflicting_area.start_addr == old_area.start_addr {
                    if old_area.end_address() < old_addr + new_size {
                        old_area.end_addr = old_addr + new_size;
                    }
                    return Ok(old_addr);
                }
            } else {
                old_area.end_addr = old_area.start_addr + new_size;
                return Ok(old_area.start_addr);
            }
        } else {
            kbail!(EFAULT, "mremap(): address not owned by task");
        }
        self.log();
        let old_area = self.area_containing(old_addr, old_addr).unwrap().clone();
        let VmemArea {
            start_addr,
            end_addr,
            flags,
            prot,
            kind: _,
        } = old_area;

        let new_addr = self.mmap(
            VirtAddr::null(),
            new_size,
            prot,
            flags,
            -1,
            0,
            active_mapper,
        )?;

        let old_pages = PageRange::new(
            Page::containing_address(start_addr),
            Page::containing_address(end_addr),
        );
        let new_pages = PageRange::new(
            Page::containing_address(new_addr),
            Page::containing_address(new_addr + old_area.size_in_bytes()),
        );
        for (old_page, new_page) in old_pages.iter().zip(new_pages.iter()) {
            let frame = active_mapper.translate(old_page.start_address());
            if let Some((frame, flags)) = frame {
                active_mapper
                    .map_to_single(new_page, Frame::containing_address(frame), flags)
                    .unwrap();
            }
            // else, do nothing?
        }
        self.munmap(active_mapper, start_addr, end_addr)?;
        Ok(new_addr)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mmap(
        &mut self,
        start_addr: VirtAddr,
        size: usize,
        protection: MMapProt,
        flags: MMapFlags,
        _fd: FileDesc,
        _offset: usize,
        active_mapper: &mut Mapper,
    ) -> KResult<VirtAddr> {
        if size == 0 {
            kbail!(EINVAL, "mmap(): size is zero");
        }
        if flags.contains(MMapFlags::MAP_FIXED) {
            if start_addr.align_down(PAGE_SIZE) != start_addr {
                kbail!(EINVAL, "mmap(): start_addr not page-aligned");
            }
            let end_addr = start_addr + size;
            if end_addr.align_up(PAGE_SIZE) != end_addr {
                kbail!(EINVAL, "mmap(): end_addr not page-aligned");
            }
            if self.area_containing(start_addr, end_addr).is_some() {
                kbail!(ENOMEM, "mmap(): address already in use");
            }

            self.map_area(
                start_addr,
                end_addr,
                flags,
                protection,
                MMapKind::Anonymous,
                active_mapper,
            )?;
            return Ok(start_addr);
        }

        let size_aligned = align_up(size, PAGE_SIZE);
        if start_addr == VirtAddr::null() {
            let start = self.find_free_space_above(USER_VALLOC_BASE, size_aligned);
            if let Some((start, prev_idx)) = start {
                if let Some(prev_idx) = prev_idx {
                    let prev = &mut self.areas[prev_idx];
                    if prev.end_addr == start && prev.prot == protection {
                        assert_eq!(prev.flags, flags);
                        assert!(matches!(prev.kind, MMapKind::Anonymous));
                        prev.end_addr = start + size_aligned;
                        return Ok(start);
                    } else {
                        log::warn!(
                            "Couldn't merge area [{:?} .. {:?}] with [{:?} .. {:?}]",
                            prev.start_addr,
                            prev.end_addr,
                            start,
                            start + size_aligned
                        );
                    }
                }

                self.add_area(
                    start,
                    start + size_aligned,
                    flags,
                    protection,
                    MMapKind::Anonymous,
                )?;
                return Ok(start);
            }

            self.log();
            kbail!(ENOMEM, "mmap(): no free space big enough");
        }

        kbail!(ENOSYS, "mmap(): not yet implemented for start_addr != null");
    }

    // pub fn brk(&mut self, active_mapper: &mut Mapper, new_brk: VirtAddr) -> KResult<VirtAddr> {
    //     let current_brk = self
    //         .areas
    //         .iter()
    //         .find(|area| matches!(area.kind, MMapKind::Anonymous))
    //         .map(|area| area.end_addr)
    //         .unwrap_or(USER_VALLOC_BASE);
    //     Ok(current_brk)
    // }

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
            if self.areas[i + 1].start_addr >= minimum_start + size
                && self.areas[i + 1].start_addr.value() - self.areas[i].end_addr.value() >= size
            {
                if self.areas[i].end_addr < minimum_start {
                    return Some((minimum_start, Some(i)));
                } else {
                    return Some((self.areas[i].end_addr, Some(i)));
                }
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
    ) -> KResult<()> {
        let count = PageRange::new(
            Page::containing_address(start_addr),
            Page::containing_address(end_addr),
        )
        .size_in_pages();
        let ap = self
            .page_allocator
            .allocate_at(Page::containing_address(start_addr), count)?;
        let _mp = active_mapper.map(ap, prot.into())?;
        self.add_area(
            start_addr.align_down(PAGE_SIZE),
            end_addr.align_up(PAGE_SIZE),
            flags,
            prot,
            kind,
        )?;
        Ok(())
    }

    unsafe fn do_unmap(
        &mut self,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
        active_mapper: &mut Mapper,
    ) -> Option<()> {
        let range = PageRange::new(
            Page::containing_address(start_addr),
            Page::containing_address(end_addr),
        );
        unsafe { self.page_allocator.insert_free_region(range) }
        for page in range.iter() {
            unsafe {
                if let Some(frame) = active_mapper.unmap_single(page) {
                    free_kernel_frames(
                        &mut AllocatedFrames::assume_allocated(FrameRange::new(frame, frame)),
                        false,
                    )
                    .ok();
                } else {
                    // log::warn!("Tried to free memory that wasn't mapped: {:?}", page);
                }
            };
        }

        // KERNEL_FRAME_ALLOCATOR
        //     .get()
        //     .unwrap()
        //     .lock()
        //     .merge_contiguous_chunks();
        Some(())
    }

    pub fn munmap(
        &mut self,
        active_mapper: &mut Mapper,
        start_addr: VirtAddr,
        end_addr: VirtAddr,
    ) -> KResult<()> {
        let area_idx = self
            .areas
            .iter_mut()
            .enumerate()
            .find(|(_idx, area)| area.contains_addr(start_addr))
            .map(|(idx, _area)| idx)
            .ok_or(kerror!(EINVAL, "munmap(): address range not owned by task"))?;
        let area_clone = self.areas[area_idx].clone();
        if start_addr <= area_clone.start_addr && end_addr >= area_clone.end_addr {
            // remove the whole area and continue recursively unmapping until the whole range is unmapped
            unsafe {
                self.do_unmap(area_clone.start_addr, area_clone.end_addr, active_mapper);
            }
            self.areas.remove(area_idx);
            self.munmap(active_mapper, area_clone.end_addr, end_addr)?;
        } else if start_addr >= area_clone.start_addr && end_addr < area_clone.end_addr {
            // split the area in two
            unsafe {
                self.do_unmap(start_addr, end_addr, active_mapper);
            }
            self.areas.remove(area_idx);
            assert!(!matches!(area_clone.kind, MMapKind::File { .. })); // todo: handle this
            self.add_area(
                area_clone.start_addr,
                start_addr,
                area_clone.flags,
                area_clone.prot,
                area_clone.kind.clone(),
            )?;
            self.add_area(
                end_addr,
                area_clone.end_addr,
                area_clone.flags,
                area_clone.prot,
                area_clone.kind,
            )?;
        } else if start_addr <= area_clone.start_addr && end_addr < area_clone.end_addr {
            // replace the end of the area (start was unmapped)
            assert!(!matches!(area_clone.kind, MMapKind::File { .. })); // todo: handle this
            unsafe {
                self.do_unmap(area_clone.start_addr, end_addr, active_mapper);
            }
            self.areas[area_idx].start_addr = end_addr;
        } else if start_addr > area_clone.start_addr && end_addr >= area_clone.end_addr {
            // replace the start of the area (end was unmapped)
            unsafe {
                self.do_unmap(start_addr, area_clone.end_addr, active_mapper);
            }
            self.areas[area_idx].end_addr = end_addr;
        } else {
            unreachable!()
        }
        Ok(())
    }

    pub fn clear(&mut self, active_mapper: &mut Mapper) {
        for id in 0..self.next_id.load(core::sync::atomic::Ordering::Acquire) {
            if let Some(area) = self.areas.get(id) {
                unsafe {
                    self.do_unmap(area.start_addr, area.end_addr, active_mapper);
                }
            }
        }
        self.areas.clear();
    }

    pub fn log(&self) {
        log::debug!("BEGIN VIRTUAL MEMORY STATE DUMP");
        for area in self.areas.iter() {
            log::debug!(
                "{:>16x?} .. {:>16x?}   | {:?}  {:?}",
                area.start_addr,
                area.end_addr,
                area.flags,
                area.prot
            );
        }
        log::debug!("END VIRTUAL MEMORY STATE DUMP");
    }

    pub fn fork_from(&mut self, parent: &Vmem) {
        self.areas = parent.areas.clone();
        // self.mp = parent.mp.clone();
        self.page_allocator = parent.page_allocator.clone();
        self.next_id.store(
            parent.next_id.load(core::sync::atomic::Ordering::Acquire),
            core::sync::atomic::Ordering::Release,
        );
    }

    pub fn handle_page_fault(
        &mut self,
        process_addr_space: &mut AddressSpace,
        faulted_addr: VirtAddr,
        stack_frame: InterruptErrorFrame,
        reason: PageFaultErrorCode,
    ) -> KResult<()> {
        let dump_and_exit = || {
            let current = current_task();
            log::error!("PID: {}", current.pid().as_usize());
            log::error!("Instruction pointer: {:#x}", { stack_frame.frame.rip });
            log::error!(
                "Process page table: {:#x}",
                process_addr_space.cr3().value()
            );
            log::error!("Current page table: {:#x}", unsafe { cr3() });
            log::error!("Faulted address: {:?}", faulted_addr);
            log::error!("Reason: {:?}", reason);
            log::debug!("{:#x?}", stack_frame);
            self.log();
            backtrace::unwind_user_stack_from(stack_frame.frame.rbp, stack_frame.frame.rip);
            get_scheduler().send_signal_to(current, SIGSEGV);
            get_scheduler().exit_current(1)
        };

        // log::debug!("User page fault at {:#x}", { stack_frame.frame.rip });
        // log::debug!("PID: {}", current_task().pid().as_usize());
        // log::debug!("Faulted address: {:?}", faulted_addr);
        // log::debug!("Reason: {:?}", reason);
        // self.log();
        // backtrace::unwind_user_stack_from(stack_frame.frame.rbp, stack_frame.frame.rip);
        if faulted_addr.align_down(PAGE_SIZE) == VirtAddr::null() {
            log::error!("User segmentation fault: null pointer access");
            dump_and_exit()
        }

        let mut faulted_area = None;
        for area in self.areas.iter() {
            if area.contains_addr(faulted_addr) {
                faulted_area = Some(area);
                break;
            }
        }

        if let Some(area) = faulted_area {
            if !reason.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
                // allocate and map pages
                let page = Page::containing_address(faulted_addr);
                let ap = self.page_allocator.allocate_at(page, 1)?;
                process_addr_space.with_mapper(|mut mapper| mapper.map(ap, area.prot.into()))?;
                match &area.kind {
                    MMapKind::Anonymous => {
                        self.zero_memory(page.start_address(), page.start_address() + PAGE_SIZE)?;
                    }
                    MMapKind::File { file, offset, size } => {
                        let size = size.min(&PAGE_SIZE);
                        let mut buf = alloc::vec![0; *size];
                        let user_buf = UserBufferMut::from_slice(&mut buf);
                        let read = file.read(*offset, user_buf, &OpenFlags::empty())?;
                        if read == 0 {
                            self.zero_memory(
                                page.start_address(),
                                page.start_address() + PAGE_SIZE,
                            )?;
                        } else {
                            unsafe {
                                page.start_address().write_bytes_user(&buf)?;
                            }
                        }
                    }
                }
                return Ok(());
            } else if reason.contains(PageFaultErrorCode::CAUSED_BY_WRITE) {
                if !area.prot.contains(MMapProt::PROT_WRITE) {
                    log::error!("User segmentation fault: illegal write");
                    dump_and_exit()
                }
                // COW
                let new_frame = alloc_kernel_frames(1)?;
                let new_page = unsafe {
                    core::slice::from_raw_parts_mut(
                        new_frame
                            .start_address()
                            .as_hhdm_virt()
                            .as_raw_ptr_mut::<u8>(),
                        PAGE_SIZE,
                    )
                };
                let old_page = unsafe {
                    core::slice::from_raw_parts(
                        faulted_addr.align_down(PAGE_SIZE).as_raw_ptr::<u8>(),
                        PAGE_SIZE,
                    )
                };
                new_page.copy_from_slice(old_page);
                process_addr_space.with_mapper(|mut mapper| -> KResult<()> {
                    unsafe {
                        mapper.unmap_single(Page::containing_address(faulted_addr));
                    }
                    mapper.map_to_single(
                        Page::containing_address(faulted_addr),
                        new_frame.start(),
                        area.prot.into(),
                    )?;
                    Ok(())
                })?;

                return Ok(());
            }
            unreachable!(
                "handle_page_fault(): faulted area found, but was already readable and writable"
            )
        } else {
            log::error!("User segmentation fault: illegal access");
            dump_and_exit()
        }

        log::warn!("Unrecoverable page fault at {:#x}", {
            stack_frame.frame.rip
        });
        kbail!(EFAULT, "handle_page_fault(): couldn't handle page fault");
    }
}

impl Default for Vmem {
    fn default() -> Self {
        Self::new()
    }
}
