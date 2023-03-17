#![no_std]
#![no_main]
#![feature(pointer_is_aligned)]

mod io;
pub mod mem;
pub mod util;

use core::sync::atomic::AtomicU64;

use limine::{LimineBootInfoRequest, LimineHhdmRequest};
use mem::addr::VirtAddr;

static BOOTLOADER_INFO: LimineBootInfoRequest = LimineBootInfoRequest::new(0);
static HHDM: LimineHhdmRequest = LimineHhdmRequest::new(0);

pub static PHYSICAL_OFFSET: AtomicU64 = AtomicU64::new(0);

#[inline]
pub fn phys_offset() -> VirtAddr {
    VirtAddr::new(PHYSICAL_OFFSET.load(core::sync::atomic::Ordering::Acquire))
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // println!("hello, world!");

    if let Some(bootinfo) = BOOTLOADER_INFO.get_response().get() {
        println!(
            "booted by {} v{}",
            bootinfo.name.to_str().unwrap().to_str().unwrap(),
            bootinfo.version.to_str().unwrap().to_str().unwrap(),
        );
    }

    // unsafe {
    PHYSICAL_OFFSET.store(
        HHDM.get_response().get().unwrap().offset,
        core::sync::atomic::Ordering::SeqCst,
    );
    // }

    println!("{:?}", phys_offset());

    hcf();
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    println!("PANIC: {}", info);
    hcf();
}

/// Die, spectacularly.
pub fn hcf() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
