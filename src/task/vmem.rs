use core::sync::atomic::AtomicUsize;

use alloc::vec::Vec;
use x86_64::structures::{idt::PageFaultErrorCode, paging::PageTableFlags};

use crate::{
    arch::idt::InterruptErrorFrame,
    backtrace, errno,
    fs::{opened_file::FileDesc, FileRef},
    kerrmsg,
    mem::{
        addr::VirtAddr,
        allocator::{
            alloc_kernel_frames, free_kernel_frames, PageAllocator, KERNEL_FRAME_ALLOCATOR,
        },
        consts::{PAGE_SIZE, USER_STACK_TOP, USER_VALLOC_BASE},
        paging::{
            mapper::Mapper,
            units::{AllocatedFrames, Frame, FrameRange, Page, PageRange},
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
        self.end_addr - self.start_addr
    }

    pub fn merge_with(&mut self, other: Self) -> KResult<()> {
        if other.size_in_bytes() == 0 {
            return Ok(());
        }
        if other.start_addr != self.end_addr && other.end_addr != self.start_addr {
            return Err(kerrmsg!("Error merging pages"));
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
                Page::containing_address(VirtAddr::new(USER_STACK_TOP)),
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
        self.areas.sort_by_key(|area| area.start_address());
        self.merge_contiguous_chunks();
        Ok(())
    }

    pub fn merge_contiguous_chunks(&mut self) {
        let mut merge1 = None;
        let mut merge2 = None;
        for (i, chunk) in self.areas.iter().enumerate() {
            if let Some(next_chunk) = self.areas.get(i + 1) {
                if chunk.flags == next_chunk.flags && chunk.prot == next_chunk.prot {
                    if chunk.start_address() == next_chunk.end_address() {
                        merge1 = Some(chunk.clone());
                        merge2 = Some(next_chunk.clone());
                        break;
                    } else if chunk.end_address() == next_chunk.start_address() {
                        merge1 = Some(next_chunk.clone());
                        merge2 = Some(chunk.clone());
                        break;
                    }
                }
            }
        }
        if let Some(ref mut merge1) = merge1 {
            let merge2 = merge2.unwrap();
            self.areas.retain(|chunk| {
                chunk.start_address() != merge1.start_address()
                    && chunk.start_address() != merge2.start_address()
            });
            if merge1.merge_with(merge2).is_err() {
                panic!("Error merging chunks");
            }
            self.areas.push(merge1.clone());
            self.areas.sort_by_key(|a| a.start_addr);
            self.merge_contiguous_chunks();
        }
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
            .area_containing_mut(start_addr, start_addr + size - 1)
            .ok_or(errno!(
                Errno::ENOMEM,
                "mprotect(): no areas containing address"
            ))?;
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
            return Err(errno!(Errno::EINVAL, "mremap(): new_size is zero"));
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
            return Err(errno!(
                Errno::EFAULT,
                "mremap(): address not owned by this process"
            ));
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

        let new_addr = self.mmap(VirtAddr::new(0), new_size, prot, flags, -1, 0)?;
        // unsafe {
        //     new_addr
        //         .as_mut_ptr::<u8>()
        //         .copy_from(start_addr.as_ptr(), old_area.size_in_bytes());
        // }
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

    pub fn mmap(
        &mut self,
        start_addr: VirtAddr,
        size: usize,
        protection: MMapProt,
        flags: MMapFlags,
        _fd: FileDesc,
        _offset: usize,
    ) -> KResult<VirtAddr> {
        if size == 0 {
            return Err(errno!(Errno::EFAULT, "mmap(): size is 0"));
        }
        if flags.contains(MMapFlags::MAP_FIXED) {
            return Err(errno!(
                Errno::ENOSYS,
                "mmap(): MMAP_FIXED not yet implemented"
            ));
        }

        let size_aligned = align_up(size, PAGE_SIZE);
        if start_addr == VirtAddr::null() {
            let start = self.find_free_space_above(VirtAddr::new(USER_VALLOC_BASE), size_aligned);
            if let Some((start, prev)) = start {
                if let Some(prev_idx) = prev {
                    let prev = &mut self.areas[prev_idx];
                    if prev.end_addr == start
                    // && prev.flags == flags
                    // todo: ???
                    && prev.prot == protection
                    // && matches!(prev.kind, MMapKind::Anonymous)
                    {
                        assert_eq!(prev.flags, flags);
                        // assert_eq!(prev.prot, protection);
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
            return Err(errno!(Errno::ENOMEM, "mmap(): no free space big enough"));
        }
        Err(errno!(
            Errno::ENOSYS,
            "mmap(): not yet implemented for start_addr != 0"
        ))
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
        let ap = self.page_allocator.allocate_range(PageRange::new(
            Page::containing_address(start_addr),
            Page::containing_address(end_addr - 1),
        ))?;
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
            Page::containing_address(end_addr - 1),
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

        KERNEL_FRAME_ALLOCATOR
            .get()
            .unwrap()
            .lock()
            .merge_contiguous_chunks();
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
            .ok_or(errno!(
                Errno::EINVAL,
                "munmap(): address range not owned by task"
            ))?;
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
        active_mapper: &mut Mapper,
        faulted_addr: VirtAddr,
        stack_frame: InterruptErrorFrame,
        reason: PageFaultErrorCode,
    ) -> KResult<()> {
        let dump_and_exit = || {
            log::error!("Instruction pointer: {:#x}", { stack_frame.frame.rip });
            log::error!("Faulted address: {:?}", faulted_addr);
            log::error!("Reason: {:?}", reason);
            log::debug!("{:#x?}", stack_frame);
            self.log();
            backtrace::unwind_user_stack_from(stack_frame.frame.rbp, stack_frame.frame.rip).ok();
            get_scheduler().send_signal_to(current_task(), SIGSEGV);
            get_scheduler().exit_current(1)
        };

        // let rip = stack_frame.frame.rip;
        // log::debug!("User page fault at {:#x}", rip);
        // log::debug!("PID: {}", current_task().pid().as_usize());
        // log::debug!("Faulted address: {:?}", faulted_addr);
        // log::debug!("Reason: {:?}", reason);
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
                let _mp = active_mapper.map(ap, area.prot.into())?;
                if !matches!(area.kind, MMapKind::File { .. }) {
                    self.zero_memory(page.start_address(), page.start_address() + PAGE_SIZE)?;
                } else {
                    todo!("map new pages to a file in page fault handler")
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
                active_mapper.map_to_single(
                    Page::containing_address(faulted_addr),
                    new_frame.start(),
                    area.prot.into(),
                )?;
                return Ok(());
            }
            unreachable!(
                "handle_page_fault(): faulted area found, but was already readable and writable"
            )
        } else {
            log::error!("User segmentation fault: illegal access");
            dump_and_exit()
        }

        Err(errno!(
            Errno::EFAULT,
            "handle_page_fault(): couldn't handle page fault"
        ))
    }
}

impl Default for Vmem {
    fn default() -> Self {
        Self::new()
    }
}
