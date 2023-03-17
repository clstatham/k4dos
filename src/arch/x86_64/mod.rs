use limine::*;
use x86_64::instructions::interrupts;
use xmas_elf::ElfFile;

use crate::mem::{self, allocator::{alloc_kernel_frames, free_kernel_frames}};

static HHDM: LimineHhdmRequest = LimineHhdmRequest::new(0);
static MEMMAP: LimineMemmapRequest = LimineMemmapRequest::new(0);
static KERNEL_FILE: LimineKernelFileRequest = LimineKernelFileRequest::new(0);
static STACK: LimineStackSizeRequest = LimineStackSizeRequest::new(0).stack_size(0x1000 * 32); // 32 pages

pub fn arch_main() {
    interrupts::disable();
    unsafe {
        let stack = STACK.get_response().as_ptr();
        core::ptr::read_volatile(stack.unwrap());
    }

    let memmap = MEMMAP.get_response().get_mut().unwrap().memmap_mut();

    crate::PHYSICAL_OFFSET.store(
        HHDM.get_response().get().unwrap().offset as usize,
        core::sync::atomic::Ordering::SeqCst,
    );

    crate::logging::init();
    log::info!("Logger initialized.");

    let kernel_file = KERNEL_FILE.get_response().get().unwrap();
    let kernel_file = kernel_file.kernel_file.get().unwrap();

    crate::backtrace::KERNEL_ELF.call_once(|| {
        let start = kernel_file.base.as_ptr().unwrap();
        let elf_slice = unsafe { core::slice::from_raw_parts(start, kernel_file.length as usize) };
        ElfFile::new(elf_slice).unwrap()
    });

    mem::allocator::init().expect("Error initializing kernel frame and page allocators");

    let mut frames = alloc_kernel_frames(1).unwrap();
    free_kernel_frames(&mut frames).unwrap();

    log::info!("It did not crash!");
}
