#![no_std]
#![no_main]
#![feature(pointer_is_aligned, panic_info_message, lang_items)]

extern crate alloc;

mod io;
#[macro_use]
pub mod serial;
pub mod arch;
pub mod backtrace;
pub mod logging;
pub mod mem;
pub mod util;

use core::sync::atomic::AtomicUsize;

use mem::addr::VirtAddr;
use x86_64::instructions::hlt;

pub static PHYSICAL_OFFSET: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn phys_offset() -> VirtAddr {
    VirtAddr::new(PHYSICAL_OFFSET.load(core::sync::atomic::Ordering::Acquire))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    arch::arch_main();

    hcf();
}

pub fn hcf() -> ! {
    loop {
        hlt();
        core::hint::spin_loop();
    }
}
