use x86_64::{
    registers::control::{Cr3, Cr3Flags},
    structures::paging::{PhysFrame, PageTableFlags},
};

use crate::util::KResult;

use super::{
    addr::{PhysAddr, VirtAddr},
    allocator::alloc_kernel_frames,
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

            // let mut mapper = Mapper::new(page_table);
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x29000)), Frame::containing_address(PhysAddr::new(0x29000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x3b000)), Frame::containing_address(PhysAddr::new(0x3b000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x35000)), Frame::containing_address(PhysAddr::new(0x35000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x3ef95000)), Frame::containing_address(PhysAddr::new(0x3ef95000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x3ef94000)), Frame::containing_address(PhysAddr::new(0x3ef94000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x3ef93000)), Frame::containing_address(PhysAddr::new(0x3ef93000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            // mapper.map_to_single(Page::containing_address(VirtAddr::new(0x3ef92000)), Frame::containing_address(PhysAddr::new(0x3ef92000)), PageTableFlags::PRESENT | PageTableFlags::WRITABLE).unwrap();
            
            // page_table[0] = active_table[0];

            frame
        };

        Ok(Self { cr3 })
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
}
