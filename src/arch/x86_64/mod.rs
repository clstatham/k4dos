use alloc::sync::Arc;
use limine::{
    LimineBootTimeRequest, LimineFramebufferRequest, LimineHhdmRequest, LimineKernelFileRequest,
    LimineMemmapRequest, LimineStackSizeRequest,
};
use x86::{
    controlregs::{self, Cr0, Cr4, Xcr0},
    cpuid::CpuId,
};
use x86_64::instructions::interrupts;

use crate::{
    backtrace,
    fs::{
        self,
        initramfs::get_root,
        opened_file::{OpenFlags, OpenedFile},
        path::Path,
    },
    god_mode::{self, GOD_MODE_FIFO},
    graphics,
    mem::{
        self,
        allocator::{KERNEL_FRAME_ALLOCATOR, KERNEL_PAGE_ALLOCATOR},
        consts::KERNEL_STACK_SIZE,
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

// static BOOT_INFO: LimineBootInfoRequest = LimineBootInfoRequest::new(0);
static HHDM: LimineHhdmRequest = LimineHhdmRequest::new(0);
static STACK: LimineStackSizeRequest =
    LimineStackSizeRequest::new(0).stack_size(KERNEL_STACK_SIZE as u64);
static BOOT_TIME: LimineBootTimeRequest = LimineBootTimeRequest::new(0);
static FB_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest::new(0);
static MEM_MAP: LimineMemmapRequest = LimineMemmapRequest::new(0);
static KERNEL_FILE: LimineKernelFileRequest = LimineKernelFileRequest::new(0);

// global_asm!(include_str!("boot.S"));

pub fn arch_main() {
    unsafe {
        core::ptr::read_volatile(STACK.get_response().as_ptr().unwrap());
    }

    interrupts::disable();

    let kernel_file = KERNEL_FILE
        .get_response()
        .as_ptr()
        .expect("Error getting kernel binary from Limine");
    let kernel_file = unsafe { &*kernel_file };
    let kernel_file = unsafe { &*kernel_file.kernel_file.as_ptr().unwrap() };
    let kernel_file_base = kernel_file.base.as_ptr().unwrap();
    let kernel_file_len = kernel_file.length as usize;
    let kernel_file_data =
        unsafe { core::slice::from_raw_parts(kernel_file_base, kernel_file_len) };
    backtrace::KERNEL_ELF.call_once(|| xmas_elf::ElfFile::new(kernel_file_data).unwrap());

    // crate::PHYSICAL_OFFSET.store(HHDM.get_response().get().unwrap().offset as usize, core::sync::atomic::Ordering::Release);
    crate::PHYSICAL_OFFSET.call_once(|| HHDM.get_response().get().unwrap().offset as usize);

    let memmap = MEM_MAP.get_response().get_mut().unwrap().memmap_mut();

    crate::logging::init();
    log::info!("Logger initialized.");

    log::info!("Setting up time structures.");
    let boot_time = unsafe { &*BOOT_TIME.get_response().as_ptr().unwrap() }.boot_time;
    time::init(boot_time);

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

    // let fb_tag = boot_info.framebuffer_tag().expect("No multiboot2 framebuffer tag found");
    let fb_resp = FB_REQUEST.get_response().get().unwrap();
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

    log::info!("Initializing VGA graphics.");

    graphics::init(fb_resp).expect("Error initializing VGA graphics");

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

    fs::devfs::init();

    log::info!("Welcome to K4DOS!");

    {
        let task = Task::new_kernel(sched, poll_serial1, true);
        sched.push_runnable(task, true);
    }

    god_mode::init();

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
        .lookup_path(Path::new("/dev/tty"), true)
        .unwrap();

    // stdin
    files
        .open_with_fd(
            0,
            Arc::new(OpenedFile::new(console.clone(), OpenFlags::O_RDONLY, 0)),
            OpenFlags::O_RDONLY | OpenFlags::O_CLOEXEC,
        )
        .unwrap();
    // stdout
    files
        .open_with_fd(
            1,
            Arc::new(OpenedFile::new(console.clone(), OpenFlags::O_WRONLY, 0)),
            OpenFlags::O_WRONLY | OpenFlags::O_CLOEXEC,
        )
        .unwrap();
    // stderr
    files
        .open_with_fd(
            2,
            Arc::new(OpenedFile::new(console, OpenFlags::O_WRONLY, 0)),
            OpenFlags::O_WRONLY | OpenFlags::O_CLOEXEC,
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
            // TTY.get().unwrap().input_char(c);
            loop {
                if let Ok(mut lock) = GOD_MODE_FIFO.get().unwrap().try_lock() {
                    lock.push_back(c);
                    drop(lock);
                    break;
                }
                // sched.preempt();
                interrupts::enable_and_hlt();
            }
        }
        interrupts::enable_and_hlt();
    }
}
