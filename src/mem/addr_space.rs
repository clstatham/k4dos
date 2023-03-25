use x86_64::{
    registers::control::{Cr3, Cr3Flags},
    structures::paging::{PageTableFlags, PhysFrame},
};

use crate::{util::KResult, vga_text};

use super::{
    addr::{PhysAddr, VirtAddr},
    allocator::alloc_kernel_frames,
    consts::PAGE_TABLE_ENTRIES,
    paging::{
        mapper::Mapper,
        table::{active_table, PageTable},
        units::{AllocatedFrames, Frame, FrameRange, Page},
    },
};

pub struct AddressSpace {
    cr3: AllocatedFrames,
}

impl AddressSpace {
    pub fn new() -> KResult<Self> {
        let cr3 = unsafe {
            let frame = alloc_kernel_frames(1)?;
            let phys_addr = frame.start_address();
            let virt_addr = phys_addr.as_hhdm_virt();

            let page_table: &mut PageTable = &mut *virt_addr.as_mut_ptr();
            let active_table = active_table();

            // zero out lower half of virtual address space
            for i in 0..256 {
                page_table[i].set_unused();
            }

            // copy kernel mappings
            for i in 256..512 {
                page_table[i] = active_table[i];
            }

            frame
        };

        let mut this = Self { cr3 };
        let mut mapper = this.mapper();
        mapper.map_to_single(
            Page::containing_address(VirtAddr::new(vga_text::VGA_BUFFER_START_PADDR)),
            Frame::containing_address(PhysAddr::new(vga_text::VGA_BUFFER_START_PADDR)),
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::NO_EXECUTE
                | PageTableFlags::USER_ACCESSIBLE,
        )?;

        Ok(this)
    }

    pub fn current() -> Self {
        let cr3 = Cr3::read().0.start_address().as_u64() as usize;
        let cr3 = PhysAddr::new(cr3);
        let cr3 = unsafe {
            AllocatedFrames::assume_allocated(FrameRange::new(
                Frame::containing_address(cr3),
                Frame::containing_address(cr3),
            ))
        };
        Self { cr3 }
    }

    pub fn cr3(&self) -> PhysAddr {
        self.cr3.start_address()
    }

    pub fn switch(&self) {
        unsafe {
            Cr3::write(
                PhysFrame::containing_address(x86_64::PhysAddr::new(
                    self.cr3.start_address().value() as u64,
                )),
                Cr3Flags::empty(),
            );
        }
    }

    pub fn mapper(&mut self) -> Mapper {
        unsafe { Mapper::new(&mut *self.cr3.start_address().as_hhdm_virt().as_mut_ptr()) }
    }

    pub fn is_active(&self) -> bool {
        self.cr3.start_address().value() == Cr3::read().0.start_address().as_u64() as usize
    }

    pub fn fork(&mut self, set_cow: bool) -> KResult<AddressSpace> {
        assert!(self.is_active(), "Can only fork the active address space");
        let mut new = AddressSpace::new()?;

        let mut my_mapper = self.mapper();
        let my_p4 = my_mapper.p4();
        let mut new_mapper = new.mapper();
        let new_p4 = new_mapper.p4();

        let mut insert_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

        if !set_cow {
            insert_flags |= PageTableFlags::WRITABLE;
        }

        for p4_idx in 0..256 {
            let my_entry = &my_p4[p4_idx];
            if my_entry.is_unused() {
                continue;
            }
            let my_p3 = my_p4.next_table(p4_idx).unwrap();
            let new_p3 = new_p4.next_table_create(p4_idx, insert_flags)?;
            for p3_idx in 0..PAGE_TABLE_ENTRIES {
                let my_entry = &my_p3[p3_idx];
                if my_entry.is_unused() {
                    continue;
                }
                let my_p2 = my_p3.next_table(p3_idx).unwrap();
                let new_p2 = new_p3.next_table_create(p3_idx, insert_flags)?;

                for p2_idx in 0..PAGE_TABLE_ENTRIES {
                    let my_entry = &my_p2[p2_idx];
                    if my_entry.is_unused() {
                        continue;
                    }
                    let my_p1 = my_p2.next_table(p2_idx).unwrap();
                    let new_p1 = new_p2.next_table_create(p2_idx, insert_flags)?;

                    for p1_idx in 0..PAGE_TABLE_ENTRIES {
                        let my_entry = &my_p1[p1_idx];
                        if my_entry.is_unused() {
                            continue;
                        }
                        let mut flags = my_entry.flags();

                        if set_cow {
                            flags.remove(PageTableFlags::WRITABLE);
                        }

                        new_p1[p1_idx].set_frame(my_entry.frame().unwrap(), flags);
                    }
                }
            }
        }

        Ok(new)
    }
}
