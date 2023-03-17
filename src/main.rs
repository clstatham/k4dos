#![no_std]
#![no_main]
#![feature(
    pointer_is_aligned, 
    panic_info_message,
    lang_items
)]

mod io;
#[macro_use]
pub mod serial;
pub mod logging;
pub mod mem;
pub mod util;
pub mod arch;
pub mod backtrace;

use core::sync::atomic::AtomicU64;

use mem::addr::VirtAddr;
use x86_64::instructions::hlt;

// static BOOTLOADER_INFO: LimineBootInfoRequest = LimineBootInfoRequest::new(0);

pub static PHYSICAL_OFFSET: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn phys_offset() -> VirtAddr {
    VirtAddr::new(PHYSICAL_OFFSET.load(core::sync::atomic::Ordering::Acquire))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // if let Some(bootinfo) = BOOTLOADER_INFO.get_response().get() {
    //     serial_println!(
    //         "booted by {} v{}",
    //         bootinfo.name.to_str().unwrap().to_str().unwrap(),
    //         bootinfo.version.to_str().unwrap().to_str().unwrap(),
    //     );
    // }

    arch::arch_main();

    hcf();
}

pub fn hcf() -> ! {
    loop {
        hlt();
        core::hint::spin_loop();
    }
}
