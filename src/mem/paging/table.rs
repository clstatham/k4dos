use core::{
    fmt::Debug,
    ops::{Index, IndexMut},
};

use x86_64::{registers::control::Cr3, structures::paging::PageTableFlags};

use crate::{
    kerr,
    mem::{
        addr::{PhysAddr, VirtAddr},
        allocator::{alloc_kernel_frames, AllocationError},
        consts::{PAGE_SIZE, PAGE_TABLE_ENTRIES},
    },
    util::{KError, KResult},
};

use super::units::Frame;

fn frame_to_table(frame: Frame) -> *mut PageTable {
    let virt = crate::phys_offset() + frame.start_address().value();
    virt.as_mut_ptr()
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry {
    data: usize,
}

impl PageTableEntry {
    const ADDRESS_MASK: usize = 0x000f_ffff_ffff_f000;
    const FLAGS_MASK: usize = 0x01ff;

    pub const fn new() -> Self {
        PageTableEntry { data: 0 }
    }

    pub const fn is_unused(&self) -> bool {
        self.data == 0
    }

    pub fn set_unused(&mut self) {
        self.data = 0
    }

    pub const fn flags(&self) -> PageTableFlags {
        PageTableFlags::from_bits_truncate(self.data as u64)
    }

    pub fn addr(&self) -> PhysAddr {
        PhysAddr::new(self.data & Self::ADDRESS_MASK)
    }

    pub fn frame(&self) -> Option<Frame> {
        if !self.flags().contains(PageTableFlags::PRESENT)
            || self.flags().contains(PageTableFlags::HUGE_PAGE)
        {
            None
        } else {
            Some(Frame::containing_address(self.addr()))
        }
    }

    pub fn set_addr(&mut self, addr: PhysAddr, flags: PageTableFlags) {
        assert!(addr.is_aligned(PAGE_SIZE));

        self.data = addr.value() | flags.bits() as usize;
    }

    pub fn set_frame(&mut self, frame: Frame, flags: PageTableFlags) {
        self.set_addr(frame.start_address(), flags)
    }

    pub fn set_flags(&mut self, flags: PageTableFlags) {
        self.data &= !Self::FLAGS_MASK;
        self.data |= flags.bits() as usize;
    }
}

impl Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PageTableEntry")
            .field("addr", &self.addr())
            .field("flags", &self.flags())
            .finish()
    }
}

#[derive(Debug)]
pub enum PagingError {
    FrameAllocationFailed(KError<AllocationError>),
    PageAllocationFailed(KError<AllocationError>),
    PageTableCreation,
    PageAlreadyMapped(Frame),
}

#[repr(C, align(4096))]
#[derive(Clone)]
pub struct PageTable {
    pub(super) entries: [PageTableEntry; PAGE_TABLE_ENTRIES],
}

impl PageTable {
    #[inline]
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::new(); PAGE_TABLE_ENTRIES],
        }
    }

    #[inline]
    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.set_unused();
        }
    }

    pub fn next_table<'b>(&self, index: usize) -> Option<&'b PageTable> {
        let ptr = frame_to_table(self[index].frame()?);
        Some(unsafe { &*ptr })
    }

    pub fn next_table_mut<'b>(&self, index: usize) -> Option<&'b mut PageTable> {
        let ptr = frame_to_table(self[index].frame()?);
        Some(unsafe { &mut *ptr })
    }

    pub fn next_table_create<'b>(
        &mut self,
        index: usize,
        insert_flags: PageTableFlags,
    ) -> KResult<&'b mut PageTable, PagingError> {
        let entry = &mut self[index];
        let created;
        if entry.is_unused() {
            match alloc_kernel_frames(1) {
                Ok(frame) => {
                    entry.set_frame(frame.start(), insert_flags);
                    created = true;
                }
                Err(e) => {
                    return Err(kerr!(
                        PagingError::FrameAllocationFailed(e),
                        "Failed to allocate frame for new page table"
                    ));
                }
            }
        } else {
            entry.set_flags(entry.flags() | insert_flags);
            created = false;
        }

        let page_table = match self.next_table_mut(index) {
            Some(pt) => pt,
            None => {
                return Err(kerr!(
                    PagingError::PageTableCreation,
                    "Could not create next page table, likely due to a huge page"
                ))
            }
        };

        if created {
            page_table.zero();
        }

        Ok(page_table)
    }
}

impl Index<usize> for PageTable {
    type Output = PageTableEntry;
    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

impl IndexMut<usize> for PageTable {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.entries[index]
    }
}

impl Debug for PageTable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // self.entries[..].fmt(f)
        writeln!(
            f,
            "PageTable[{:?}]",
            VirtAddr::new(self as *const _ as usize).as_hhdm_phys()
        )?;
        for (i, entry) in self.entries.iter().enumerate() {
            if !entry.is_unused() {
                writeln!(f, "{:>3}: {:>16?} | {:?}", i, entry.addr(), entry.flags())?;
            }
        }
        Ok(())
    }
}

pub fn active_table() -> &'static mut PageTable {
    let cr3 = PhysAddr::new(Cr3::read().0.start_address().as_u64() as usize);
    unsafe { &mut *cr3.as_hhdm_virt().as_mut_ptr() }
}
