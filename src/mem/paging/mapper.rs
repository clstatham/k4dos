use x86::tlb;
use x86_64::structures::paging::PageTableFlags;

use crate::{
    kerr,
    mem::{
        addr::{PhysAddr, VirtAddr},
        allocator::alloc_kernel_frames,
    },
    util::KResult,
};

use super::{
    table::{PageTable, PagingError},
    units::{AllocatedFrames, AllocatedPages, Frame, MappedPages, Page},
};

#[derive(Debug)]
#[must_use = "Changes to page tables must be flushed or ignored."]
pub struct PageFlush(Page);

impl PageFlush {
    pub fn new(page: Page) -> Self {
        PageFlush(page)
    }

    pub fn ignore(self) {}

    pub fn flush(self) {
        unsafe { tlb::flush(self.0.start_address().value()) }
    }
}

#[derive(Debug)]
pub struct Mapper<'a> {
    p4: &'a mut PageTable,
}

impl<'a> Mapper<'a> {
    pub unsafe fn new(p4: &'a mut PageTable) -> Self {
        Self { p4 }
    }

    pub fn translate(&self, addr: VirtAddr) -> Option<(PhysAddr, PageTableFlags)> {
        let p3 = self.p4.next_table(addr.p4_index())?;
        let p2 = p3.next_table(addr.p3_index())?;
        let p1 = p2.next_table(addr.p2_index())?;
        let entry = p1[addr.p1_index()];

        Some((entry.addr(), entry.flags()))
    }

    pub fn map_to_single(
        &mut self,
        page: Page,
        frame: Frame,
        flags: PageTableFlags,
    ) -> KResult<(), PagingError> {
        let mut insert_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        if flags.contains(PageTableFlags::USER_ACCESSIBLE) {
            insert_flags |= PageTableFlags::USER_ACCESSIBLE;
        }
        let addr = page.start_address();

        let p3 = self.p4.next_table_create(addr.p4_index(), insert_flags)?;
        let p2 = p3.next_table_create(addr.p3_index(), insert_flags)?;
        let p1 = p2.next_table_create(addr.p2_index(), insert_flags)?;
        let entry = &mut p1[addr.p1_index()];
        if !entry.is_unused() {
            return Err(kerr!(PagingError::PageAlreadyMapped(
                entry.frame().unwrap()
            )));
        }
        entry.set_frame(frame, flags);
        unsafe { tlb::flush(addr.value()) }
        Ok(())
    }

    pub fn map_to(
        &mut self,
        pages: AllocatedPages,
        frames: AllocatedFrames,
        flags: PageTableFlags,
    ) -> KResult<MappedPages, PagingError> {
        assert_eq!(
            pages.size_in_pages(),
            frames.size_in_pages(),
            "Number of pages must equal number of frames"
        );
        for (page, frame) in pages.iter().zip(frames.iter()) {
            self.map_to_single(page, frame, flags)?;
        }

        Ok(unsafe { MappedPages::assume_mapped(pages, frames, flags) })
    }

    pub fn map(
        &mut self,
        pages: AllocatedPages,
        flags: PageTableFlags,
    ) -> KResult<MappedPages, PagingError> {
        let frames = alloc_kernel_frames(pages.size_in_pages())
            .map_err(|e| kerr!(PagingError::FrameAllocationFailed(e)))?;
        self.map_to(pages, frames, flags)
    }
}
