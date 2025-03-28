use super::addr::VirtAddr;

pub const PAGE_SHIFT: usize = 12;
pub const PAGE_SIZE: usize = 0x1000;
pub const PAGE_TABLE_ENTRIES: usize = 512;
pub const L4_SHIFT: usize = 39;
pub const L3_SHIFT: usize = 30;
pub const L2_SHIFT: usize = 21;
pub const L1_SHIFT: usize = 12;

pub const KERNEL_STACK_SIZE: usize = PAGE_SIZE * 32;
pub const USER_STACK_SIZE: usize = PAGE_SIZE * 64;

/// The maximum canonical virtual address in low (user) address space.
/// All user virtual addresses are less than this value.
pub const MAX_LOW_VADDR: VirtAddr = unsafe { VirtAddr::new_unchecked(0x0000_7000_0000_0000) };

/// The minimum canonical virtual address in high (kernel) address space.
/// All kernel virtual addresses are greater than or equal to this value.
pub const MIN_HIGH_VADDR: VirtAddr = unsafe { VirtAddr::new_unchecked(0xffff_8000_0000_0000) };

pub const USER_VALLOC_BASE: VirtAddr = unsafe { VirtAddr::new_unchecked(0x0000_000a_0000_0000) };
pub const USER_VALLOC_END: VirtAddr = unsafe { VirtAddr::new_unchecked(0x0000_0fff_0000_0000) };
pub const USER_STACK_TOP: VirtAddr = unsafe { VirtAddr::new_unchecked(0x0000_0fff_ffff_e000) };
pub const USER_STACK_BOTTOM: VirtAddr = USER_STACK_TOP.const_sub(USER_STACK_SIZE);

pub const KERNEL_HEAP_START: VirtAddr = unsafe { VirtAddr::new_unchecked(0xFFFF_FE80_0000_0000) };
pub const KERNEL_HEAP_SIZE: usize = 1024 * 1024 * 1024; // 1024 MiB
