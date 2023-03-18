use lazy_static::lazy_static;
use pc_keyboard::{layouts::Us104Key, DecodedKey, HandleControl, Keyboard, ScancodeSet1};


use x2apic::lapic::{LocalApicBuilder, LocalApic, xapic_base};
use x86_64::{
    instructions::port::Port,
    registers::control::Cr3,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
};

use crate::{util::SpinLock, task::get_scheduler};

use super::cpu_local::kpcr;

// pub const PIC_1_OFFSET: u8 = 32;
// pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

// pub const KEYBOARD_IDT_IDX: u8 = 33;

// pub static PICS: Mutex<ChainedPics> =
//     Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

pub const TIMER_IRQ: usize = 32;
pub const ERROR_IRQ: usize = 33;
pub const SPURIOUS_IRQ: usize = 34;

lazy_static! {
    pub static ref LAPIC: SpinLock<LocalApic> = SpinLock::new(LocalApicBuilder::new()
        .timer_vector(TIMER_IRQ)
        .error_vector(ERROR_IRQ)
        .spurious_vector(SPURIOUS_IRQ)
        .set_xapic_base(unsafe { xapic_base()} )
        .build()
        .unwrap());
}

#[derive(Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct InterruptFrame {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rdi: u64,
    pub error: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub fn notify_eoi() {
    // unsafe { PICS.lock().notify_end_of_interrupt(index) }
    unsafe { LAPIC.lock().end_of_interrupt() }
}

pub fn init() {
    // {
    // let idt = IDT.get_mut()?;
    let mut idt = InterruptDescriptorTable::new();
    idt.divide_error.set_handler_fn(divide_error_handler);
    idt.debug.set_handler_fn(debug_handler);
    idt.non_maskable_interrupt.set_handler_fn(nmi_handler);
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.overflow.set_handler_fn(overflow_handler);
    idt.bound_range_exceeded
        .set_handler_fn(bound_range_exceeded_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.device_not_available
        .set_handler_fn(device_not_available_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(0);
    }

    // reserved: 0x09 coprocessor segment overrun exception
    idt.invalid_tss.set_handler_fn(invalid_tss_handler);
    idt.segment_not_present
        .set_handler_fn(segment_not_present_handler);
    idt.stack_segment_fault
        .set_handler_fn(stack_segment_fault_handler);
    idt.general_protection_fault
        .set_handler_fn(general_protection_fault_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    // reserved: 0x0F
    idt.x87_floating_point
        .set_handler_fn(x87_floating_point_handler);
    idt.alignment_check.set_handler_fn(alignment_check_handler);
    idt.machine_check.set_handler_fn(machine_check_handler);
    idt.simd_floating_point
        .set_handler_fn(simd_floating_point_handler);
    idt.virtualization.set_handler_fn(virtualization_handler);
    // reserved: 0x15 - 0x1C
    idt.vmm_communication_exception
        .set_handler_fn(vmm_communication_exception_handler);
    idt.security_exception
        .set_handler_fn(security_exception_handler);

    idt[TIMER_IRQ].set_handler_fn(timer_handler);
    idt[ERROR_IRQ].set_handler_fn(lapic_error_handler);
    idt[SPURIOUS_IRQ].set_handler_fn(lapic_spurious_handler);
    // idt[KEYBOARD_IDT_IDX as usize].set_handler_fn(keyboard_handler);

    kpcr().idt = idt;
    kpcr().idt.load();
    // };
    unsafe {
        let mut lock = LAPIC.lock();
        lock.enable();
        // lock.enable_timer();
    }
}

extern "x86-interrupt" fn lapic_error_handler(stack_frame: InterruptStackFrame) {
    panic!("LOCAL APIC ERROR: {:#x?}", stack_frame)
}

extern "x86-interrupt" fn lapic_spurious_handler(_stack_frame: InterruptStackFrame) {
    notify_eoi();
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    lazy_static! {
        static ref KEYBOARD: SpinLock<Keyboard<Us104Key, ScancodeSet1>> = SpinLock::new(
            Keyboard::new(ScancodeSet1::new(), Us104Key, HandleControl::Ignore)
        );
    }

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    let mut keyboard = KEYBOARD.lock();
    if let Ok(Some(key_evt)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_evt) {
            let c = match key {
                DecodedKey::Unicode(c) => c,
                DecodedKey::RawKey(_code) => '?',
            };

            // todo: PTY emulation!
            crate::terminal_print!("{}", c);
            // TTY.get().unwrap().input_char(c as u8);
        }
    }

    notify_eoi();
}

/// exception 0x00
extern "x86-interrupt" fn divide_error_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: DIVIDE ERROR\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x01
extern "x86-interrupt" fn debug_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: DEBUG EXCEPTION\n{:#x?}", stack_frame);
    // log::debug!("{:#x}", debug::Dr7::read().bits());
    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x02
extern "x86-interrupt" fn nmi_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: NON-MASKABLE INTERRUPT\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x03
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: BREAKPOINT\n{:#x?}", stack_frame);
    // don't halt here, this isn't a fatal/permanent failure, just a brief pause.
}

/// exception 0x04
extern "x86-interrupt" fn overflow_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: OVERFLOW\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x05
extern "x86-interrupt" fn bound_range_exceeded_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: BOUND RANGE EXCEEDED\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x06
extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: INVALID OPCODE\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x07
///
/// For more information about "spurious interrupts",
/// see [here](http://wiki.osdev.org/I_Cant_Get_Interrupts_Working#I_keep_getting_an_IRQ7_for_no_apparent_reason).
extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: DEVICE NOT AVAILABLE\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x08
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    log::error!(
        "\nEXCEPTION: DOUBLE FAULT\n{:#x?}\nError code: {:#b}\ncr3: {:#x}",
        stack_frame,
        error_code,
        Cr3::read_raw().0.start_address().as_u64()
    );

    log::error!("\nNote: this may be caused by stack overflow.");
    panic!()
}

/// exception 0x0A
extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    log::error!(
        "\nEXCEPTION: INVALID TSS\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    panic!()
}

/// exception 0x0B
extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "\nEXCEPTION: SEGMENT NOT PRESENT\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    panic!()
}

/// exception 0x0C
extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "\nEXCEPTION: STACK SEGMENT FAULT\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    panic!()
}

/// exception 0x0D
extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "\nEXCEPTION: GENERAL PROTECTION FAULT\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    // backtrace::backtrace();
    panic!()
}

/// exception 0x0E
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    let accessed_address = x86_64::registers::control::Cr2::read_raw();
    let cr3 = x86_64::registers::control::Cr3::read_raw().0;
    // if error_code.contains(PageFaultErrorCode::USER_MODE) {
    //     unsafe {
    //         core::arch::asm!("swapgs");
    //     }
    //     handle_user_page_fault(
    //         accessed_address,
    //         error_code,
    //         stack_frame,
    //         cr3.start_address().as_u64(),
    //     );
    //     unsafe {
    //         core::arch::asm!("swapgs");
    //     }
    //     return;
    // }

    log::error!(
        "\nEXCEPTION: PAGE FAULT while accessing {:#x}\n\
        error code: {:?}\ncr3: {:#x}\n{:#x?}",
        accessed_address,
        error_code,
        cr3.start_address().as_u64(),
        stack_frame,
    );

    log::error!("Exception IP {:#x}", stack_frame.instruction_pointer);
    log::error!("Faulted access address {:#x}", accessed_address,);
    panic!()
}

/// exception 0x10
extern "x86-interrupt" fn x87_floating_point_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: x87 FLOATING POINT\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x11
extern "x86-interrupt" fn alignment_check_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "\nEXCEPTION: ALIGNMENT CHECK\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    panic!()
}

/// exception 0x12
extern "x86-interrupt" fn machine_check_handler(stack_frame: InterruptStackFrame) -> ! {
    log::error!("\nEXCEPTION: MACHINE CHECK\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x13
extern "x86-interrupt" fn simd_floating_point_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: SIMD FLOATING POINT\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x14
extern "x86-interrupt" fn virtualization_handler(stack_frame: InterruptStackFrame) {
    log::error!("\nEXCEPTION: VIRTUALIZATION\n{:#x?}", stack_frame);
    panic!()
}

/// exception 0x1D
extern "x86-interrupt" fn vmm_communication_exception_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "\nEXCEPTION: VMM COMMUNICATION EXCEPTION\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    panic!()
}

/// exception 0x1E
extern "x86-interrupt" fn security_exception_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    log::error!(
        "\nEXCEPTION: SECURITY EXCEPTION\n{:#x?}\nError code: {:#b}",
        stack_frame,
        error_code
    );
    panic!()
}

extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    notify_eoi();
    // get_scheduler().preempt();
}
