use limine::*;
use x86_64::instructions::interrupts;

static HHDM: LimineHhdmRequest = LimineHhdmRequest::new(0);
static MEMMAP: LimineMemmapRequest = LimineMemmapRequest::new(0);
static KERNEL_FILE: LimineKernelFileRequest = LimineKernelFileRequest::new(0);
static STACK: LimineStackSizeRequest = LimineStackSizeRequest::new(0).stack_size(0x1000 * 32); // 32 pages

pub fn arch_main() {
    unsafe {
        core::ptr::read_volatile(STACK.get_response().as_ptr().unwrap());
    }

    let memmap = MEMMAP.get_response().get_mut().unwrap().memmap_mut();

    interrupts::disable();

    crate::PHYSICAL_OFFSET.store(
        HHDM.get_response().get().unwrap().offset,
        core::sync::atomic::Ordering::SeqCst,
    );

    crate::logging::init();

    log::info!("Logger initialized.");    
}
