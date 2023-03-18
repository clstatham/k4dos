#![no_std]
#![no_main]
#![feature(
    pointer_is_aligned,
    panic_info_message,
    lang_items,
    abi_x86_interrupt,
    naked_functions,
    asm_const,
    ptr_internals
)]

extern crate alloc;

mod io;
#[macro_use]
pub mod serial;
pub mod arch;
pub mod backtrace;
pub mod logging;
pub mod mem;
pub mod task;
pub mod util;
pub mod fs;
pub mod userland;

use core::sync::atomic::AtomicUsize;

use mem::addr::VirtAddr;
use x86_64::instructions::hlt;

use crate::{task::get_scheduler, fs::initramfs};

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

pub fn main_kernel_thread() {
    log::info!("We are now in main_kernel_thread().");

    fs::initramfs::init().unwrap();

    let sched = get_scheduler();
    loop {
        sched.preempt();
        // core::hint::spin_loop();
    }
}

pub fn hcf() -> ! {
    loop {
        hlt();
        core::hint::spin_loop();
    }
}
