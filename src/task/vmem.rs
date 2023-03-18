use alloc::vec::Vec;
use x86_64::structures::paging::PageTableFlags;

use crate::mem::addr::VirtAddr;

#[derive(Clone)]
pub struct VmemArea {
    start_addr: VirtAddr,
    end_addr: VirtAddr,
    flags: PageTableFlags,
}

impl VmemArea {
    pub const fn new(start_addr: VirtAddr, end_addr: VirtAddr, flags: PageTableFlags) -> Self {
        Self {
            start_addr,
            end_addr,
            flags,
        }
    }
}

pub struct Vmem {
    areas: Vec<VmemArea>,
}

impl Vmem {
    pub const fn new() -> Self {
        Self { areas: Vec::new() }
    }
}
