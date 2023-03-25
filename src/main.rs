#![no_std]
#![no_main]
#![feature(
    pointer_is_aligned,
    panic_info_message,
    lang_items,
    abi_x86_interrupt,
    naked_functions,
    asm_const,
    ptr_internals,
    const_refs_to_cell,
    slice_pattern,
    is_sorted,
    map_try_insert
)]
#![allow(clippy::missing_safety_doc)]

extern crate alloc;

#[macro_use]
pub mod vga_text;
#[macro_use]
pub mod serial;
pub mod arch;
pub mod backtrace;
pub mod fs;
pub mod logging;
pub mod mem;
pub mod task;
pub mod userland;
pub mod util;



use mem::addr::VirtAddr;

use x86_64::instructions::hlt;

// pub static PHYSICAL_OFFSET: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn phys_offset() -> VirtAddr {
    // VirtAddr::new(PHYSICAL_OFFSET.load(core::sync::atomic::Ordering::Acquire))
    VirtAddr::new(0xffff800000000000)
}

#[no_mangle]
pub extern "C" fn kernel_main(_magic: u64, boot_info_addr: usize) -> ! {
    let boot_info = unsafe { multiboot2::load(boot_info_addr) }.unwrap();
    arch::arch_main(boot_info);

    hcf();
}

pub fn hcf() -> ! {
    loop {
        hlt();
        core::hint::spin_loop();
    }
}
