use alloc::sync::Arc;
use limine::request::{
    BootTimeRequest, FramebufferRequest, HhdmRequest, KernelFileRequest, MemoryMapRequest,
    StackSizeRequest,
};

use spin::Once;
use x86::{
    controlregs::{self, Cr0, Cr4},
    cpuid::{CpuId, FeatureInfo},
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
    god_mode::GOD_MODE_FIFO,
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

static HHDM: HhdmRequest = HhdmRequest::new();
static _STACK: StackSizeRequest = StackSizeRequest::new().with_size(KERNEL_STACK_SIZE as u64);
static BOOT_TIME: BootTimeRequest = BootTimeRequest::new();
static FB_REQUEST: FramebufferRequest = FramebufferRequest::new();
static MEM_MAP: MemoryMapRequest = MemoryMapRequest::new();
static KERNEL_FILE: KernelFileRequest = KernelFileRequest::new();

pub static CPUID_FEATURE_INFO: Once<FeatureInfo> = Once::new();

pub fn get_cpuid_feature_info() -> &'static FeatureInfo {
    CPUID_FEATURE_INFO.call_once(|| {
        CpuId::new()
            .get_feature_info()
            .expect("Error getting CPUID feature info")
    })
}

pub fn arch_main() {
    interrupts::disable();

    let kernel_file = KERNEL_FILE
        .get_response()
        .expect("Error getting kernel binary from Limine");
    let kernel_file = kernel_file.file();
    let kernel_file_base = kernel_file.addr();
    let kernel_file_len = kernel_file.size() as usize;
    let kernel_file_data =
        unsafe { core::slice::from_raw_parts(kernel_file_base, kernel_file_len) };
    backtrace::KERNEL_ELF.call_once(|| {
        xmas_elf::ElfFile::new(kernel_file_data).expect("Error parsing kernel ELF file data")
    });

    crate::PHYSICAL_OFFSET.call_once(|| {
        HHDM.get_response()
            .expect("Error getting HHDM response from Limine")
            .offset() as usize
    });

    let memmap = MEM_MAP
        .get_response()
        .expect("Error getting memory map response from Limine")
        .entries();

    crate::logging::init();
    log::info!("Logger initialized.");

    log::info!("Setting up time structures.");
    let boot_time = BOOT_TIME
        .get_response()
        .expect("Error getting boot time response from Limine")
        .boot_time();
    time::init(boot_time.as_secs() as i64);

    log::info!("Initializing FPU mechanisms.");
    let features = get_cpuid_feature_info();
    assert!(features.has_fxsave_fxstor(), "FXSAVE/FXRSTOR not available");
    assert!(features.has_mmx(), "MMX not available");
    assert!(features.has_fpu(), "FPU not available");
    assert!(features.has_sse(), "SSE not available");
    unsafe {
        // enable FXSAVE and FXRSTOR
        controlregs::cr4_write(controlregs::cr4() | Cr4::CR4_ENABLE_SSE | Cr4::CR4_UNMASKED_SSE);
        log::trace!("CR4_ENABLE_SSE and CR4_UNMASKED_SSE set.");

        // controlregs::xcr0_write(
        //     controlregs::xcr0()
        //         | Xcr0::XCR0_SSE_STATE
        //         | Xcr0::XCR0_FPU_MMX_STATE
        //         | Xcr0::XCR0_AVX_STATE,
        // );
        // log::trace!("XCR0_SSE_STATE, XCR0_FPU_MMX_STATE, and XCR0_AVX_STATE set.");
        controlregs::cr0_write(controlregs::cr0() & !Cr0::CR0_EMULATE_COPROCESSOR);
        log::trace!("CR0_EMULATE_COPROCESSOR cleared.");
        controlregs::cr0_write(controlregs::cr0() | Cr0::CR0_MONITOR_COPROCESSOR);
        log::trace!("CR0_MONITOR_COPROCESSOR set.");
    }

    log::info!("Initializing boot GDT.");
    gdt::init_boot();

    // let fb_tag = boot_info.framebuffer_tag().expect("No multiboot2 framebuffer tag found");
    let fb_resp = FB_REQUEST
        .get_response()
        .expect("Error getting framebuffer response from Limine");
    log::info!("Initializing kernel frame and page allocators.");
    mem::allocator::init(memmap).expect("Error initializing kernel frame and page allocators");

    log::info!("Remapping kernel to new page table.");
    let kernel_addr_space = mem::remap_kernel().expect("Error remapping kernel");

    log::info!("Setting up kernel heap.");
    let _heap_mp = kernel_addr_space
        .lock()
        .with_mapper(|mut mapper| mem::init_heap(&mut mapper).expect("Error setting up heap"));

    log::info!("Converting kernel frame and page allocators to use heap.");
    {
        KERNEL_FRAME_ALLOCATOR
            .get()
            .expect("Error getting kernel frame allocator")
            .lock()
            .convert_to_heap_allocated();
        KERNEL_PAGE_ALLOCATOR
            .get()
            .expect("Error getting kernel page allocator")
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
    fs::initramfs::init().expect("Error initializing initramfs");

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

    // god_mode::init();

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

pub fn idle() {
    loop {
        interrupts::enable_and_hlt();
    }
}
