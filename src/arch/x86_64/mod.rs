use limine::*;
use x86_64::instructions::interrupts;
use xmas_elf::ElfFile;

use crate::{
    fs::{self, initramfs::get_root, path::Path},
    mem::{
        self,
        allocator::{KERNEL_FRAME_ALLOCATOR, KERNEL_PAGE_ALLOCATOR},
    },
    task::{get_scheduler, Task},
};

pub mod cpu_local;
pub mod gdt;
pub mod idt;
pub mod syscall;
pub mod task;

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

    log::info!("Initializing boot GDT.");
    gdt::init_boot();

    log::info!("Initializing kernel frame and page allocators.");
    mem::allocator::init(memmap).expect("Error initializing kernel frame and page allocators");

    log::info!("Remapping kernel to new page table.");
    let mut kernel_addr_space = mem::remap_kernel().expect("Error remapping kernel");

    log::info!("Setting up kernel heap.");
    let _heap_mp = mem::init_heap(&mut kernel_addr_space.mapper()).expect("Error setting up heap");

    log::info!("Converting kernel frame and page allocators to use heap.");
    {
        KERNEL_FRAME_ALLOCATOR
            .get()
            .unwrap()
            .lock()
            .convert_to_heap_allocated();
        KERNEL_PAGE_ALLOCATOR
            .get()
            .unwrap()
            .lock()
            .convert_to_heap_allocated();
    }

    log::info!("Setting up syscalls.");
    unsafe {
        syscall::init();
    }

    log::info!("Loading GDT.");
    gdt::init();

    log::info!("Loading IDT.");
    idt::init();

    log::info!("Initializing task scheduler.");
    crate::task::init();

    log::info!("Initializing filesystems.");
    fs::initramfs::init().unwrap();

    log::info!("Starting init process.");

    let sched = get_scheduler();
    // sched.enqueue(Task::new_kernel(spawn_init_process, true));
    let exe = "/bin/sh";
    let file = get_root()
        .unwrap()
        .lookup(Path::new(exe))
        .unwrap()
        .as_file()
        .unwrap()
        .clone();
    sched.enqueue(Task::new_init(file, &[&[]], &[&[]]).unwrap());

    log::info!("Welcome to K4DOS!");
    loop {
        interrupts::enable_and_hlt();
        interrupts::disable();
        sched.preempt();
    }
}
