use core::{arch::global_asm};

use alloc::sync::Arc;
use multiboot2::BootInformation;
use x86::{
    controlregs::{self, Cr0, Cr4, Xcr0},
    cpuid::CpuId,
};
use x86_64::instructions::{hlt, interrupts};

use crate::{
    fs::{
        self,
        initramfs::get_root,
        opened_file::{OpenFlags, OpenOptions, OpenedFile},
        path::Path,
        tty::TTY,
    },
    mem::{
        self,
        allocator::{KERNEL_FRAME_ALLOCATOR, KERNEL_PAGE_ALLOCATOR},
    },
    serial::serial1_recv,
    task::{current_task, get_scheduler, Task},
};

pub mod cpu_local;
pub mod gdt;
pub mod idt;
pub mod syscall;
pub mod task;
pub mod time;

global_asm!(include_str!("boot.S"));

pub fn arch_main(boot_info: BootInformation) {
    interrupts::disable();

    let memmap = boot_info.memory_map_tag().unwrap();

    crate::logging::init();
    log::info!("Logger initialized.");

    log::info!("Setting up time structures.");
    time::init();

    log::info!("Initializing FPU mechanisms.");
    let features = CpuId::new().get_feature_info().unwrap();
    assert!(features.has_xsave(), "XSAVE not available");
    assert!(features.has_mmx(), "MMX not available");
    assert!(features.has_fpu(), "FPU not available");
    assert!(features.has_sse(), "SSE not available");
    unsafe {
        controlregs::cr4_write(controlregs::cr4() | Cr4::CR4_ENABLE_OS_XSAVE);
        x86_64::registers::control::Cr4::write_raw(
            x86_64::registers::control::Cr4::read_raw() | (3 << 9),
        );
        controlregs::xcr0_write(
            controlregs::xcr0()
                | Xcr0::XCR0_SSE_STATE
                | Xcr0::XCR0_FPU_MMX_STATE
                | Xcr0::XCR0_AVX_STATE,
        );
        controlregs::cr0_write(controlregs::cr0() & !Cr0::CR0_EMULATE_COPROCESSOR);
        controlregs::cr0_write(controlregs::cr0() | Cr0::CR0_MONITOR_COPROCESSOR);
    }

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

    log::info!("Initializing filesystems.");
    fs::initramfs::init().unwrap();

    log::info!("Initializing task scheduler.");
    crate::task::init();

    log::info!("Starting init process.");

    let sched = get_scheduler();

    fs::null::init();
    fs::tty::init();

    log::info!("Welcome to K4DOS!");

    {
        let task = Task::new_kernel(sched, poll_serial1, true);
        sched.push_runnable(task);
    }

    loop {
        interrupts::enable_and_hlt();
    }
}

pub fn startup_init() {
    let exe = "/bin/sh";
    let file = get_root()
        .unwrap()
        .lookup(Path::new(exe), true)
        .unwrap()
        .as_file()
        .unwrap()
        .clone();

    let current = current_task();
    let mut files = current.opened_files.lock();

    let console = get_root()
        .unwrap()
        .lookup_path(Path::new("/dev/console"), true)
        .unwrap();

    // stdin
    files
        .open_with_fd(
            0,
            Arc::new(OpenedFile::new(
                console.clone(),
                OpenFlags::O_RDONLY.into(),
                0,
            )),
            OpenOptions::new(true, false),
        )
        .unwrap();
    // stdout
    files
        .open_with_fd(
            1,
            Arc::new(OpenedFile::new(
                console.clone(),
                OpenFlags::O_WRONLY.into(),
                0,
            )),
            OpenOptions::new(true, false),
        )
        .unwrap();
    // stderr
    files
        .open_with_fd(
            2,
            Arc::new(OpenedFile::new(console, OpenFlags::O_WRONLY.into(), 0)),
            OpenOptions::new(true, false),
        )
        .unwrap();
    drop(files);
    current
        .exec(file, &[exe.as_bytes()], &[b"FOO=bar"])
        .unwrap();
}

fn poll_serial1() {
    loop {
        let c = serial1_recv();
        if let Some(c) = c {
            TTY.get().unwrap().input_char(c);
        }
        hlt();
    }
}
