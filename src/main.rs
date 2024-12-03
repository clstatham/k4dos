#![no_std]
#![no_main]
#![feature(
    lang_items,
    abi_x86_interrupt,
    naked_functions,
    ptr_internals,
    slice_pattern,
    map_try_insert,
    iter_advance_by,
    alloc_error_handler
)]
#![allow(internal_features)]
#![allow(clippy::missing_safety_doc)]
// #![warn(clippy::unwrap_used)]

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
#[macro_use]
pub mod graphics;
pub mod god_mode;

use mem::addr::VirtAddr;

use spin::Once;
use x86_64::instructions::hlt;

pub static PHYSICAL_OFFSET: Once<usize> = Once::new();

#[inline]
pub fn phys_offset() -> VirtAddr {
    VirtAddr::new(*PHYSICAL_OFFSET.get().unwrap())
    // VirtAddr::new(0xffff800000000000)
}

#[no_mangle]
pub extern "C" fn start() -> ! {
    // let boot_info = unsafe { multiboot2::load(boot_info_addr) }.unwrap();
    arch::arch_main();

    hcf();
}

pub fn hcf() -> ! {
    loop {
        hlt();
        core::hint::spin_loop();
    }
}
