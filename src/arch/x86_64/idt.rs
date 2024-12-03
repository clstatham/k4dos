use lazy_static::lazy_static;

use pc_keyboard::{layouts::Us104Key, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use pic8259::ChainedPics;
use spin::Mutex;

use x86::{
    io::outb,
    msr::{rdmsr, IA32_FS_BASE},
};
use x86_64::{
    instructions::port::Port,
    registers::control::Cr3,
    structures::idt::{InterruptDescriptorTable, PageFaultErrorCode},
};

use crate::{
    backtrace,
    fs::devfs::{input::KBD_DEVICE, tty::TTY},
    mem::addr::VirtAddr,
    task::get_scheduler,
    util::IrqMutex,
};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

pub const TIMER_IRQ: u8 = PIC_1_OFFSET;
pub const KEYBOARD_IRQ: u8 = PIC_1_OFFSET + 1;
pub const COM2_IRQ: u8 = PIC_1_OFFSET + 3;

lazy_static! {
    pub static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        #[allow(clippy::fn_to_numeric_cast)]
        unsafe {
            idt.divide_error.set_handler_addr(x86_64::VirtAddr::new(divide_error_handler as u64));
            idt.debug.set_handler_addr(x86_64::VirtAddr::new(debug_handler as u64));
            idt.non_maskable_interrupt.set_handler_addr(x86_64::VirtAddr::new(nmi_handler as u64));
            idt.breakpoint.set_handler_addr(x86_64::VirtAddr::new(breakpoint_handler as u64));
            idt.overflow.set_handler_addr(x86_64::VirtAddr::new(overflow_handler as u64));
            idt.bound_range_exceeded
            .set_handler_addr(x86_64::VirtAddr::new(bound_range_exceeded_handler as u64));
            idt.invalid_opcode.set_handler_addr(x86_64::VirtAddr::new(invalid_opcode_handler as u64));
            idt.device_not_available
            .set_handler_addr(x86_64::VirtAddr::new(device_not_available_handler as u64));
            idt.double_fault
            .set_handler_addr(x86_64::VirtAddr::new(double_fault_handler as u64))
                .set_stack_index(0);

            // reserved: 0x09 coprocessor segment overrun exception
            idt.invalid_tss.set_handler_addr(x86_64::VirtAddr::new(invalid_tss_handler as u64));
            idt.segment_not_present
            .set_handler_addr(x86_64::VirtAddr::new(segment_not_present_handler as u64));
            idt.stack_segment_fault
            .set_handler_addr(x86_64::VirtAddr::new(stack_segment_fault_handler as u64));
            idt.general_protection_fault
            .set_handler_addr(x86_64::VirtAddr::new(general_protection_fault_handler as u64));
            idt.page_fault.set_handler_addr(x86_64::VirtAddr::new(page_fault_handler as u64));
            // reserved: 0x0F
            idt.x87_floating_point
            .set_handler_addr(x86_64::VirtAddr::new(x87_floating_point_handler as u64));
            idt.alignment_check.set_handler_addr(x86_64::VirtAddr::new(alignment_check_handler as u64));
            idt.machine_check.set_handler_addr(x86_64::VirtAddr::new(machine_check_handler as u64));
            idt.simd_floating_point
            .set_handler_addr(x86_64::VirtAddr::new(simd_floating_point_handler as u64));
            idt.virtualization.set_handler_addr(x86_64::VirtAddr::new(virtualization_handler as u64));
            // reserved: 0x15 - 0x1C
            idt.vmm_communication_exception
            .set_handler_addr(x86_64::VirtAddr::new(vmm_communication_exception_handler as u64));
            idt.security_exception
            .set_handler_addr(x86_64::VirtAddr::new(security_exception_handler as u64));

            idt[TIMER_IRQ].set_handler_addr(x86_64::VirtAddr::new(timer_handler as u64));
            idt[KEYBOARD_IRQ].set_handler_addr(x86_64::VirtAddr::new(keyboard_handler as u64));
            idt[COM2_IRQ].set_handler_addr(x86_64::VirtAddr::new(com2_handler as u64));
        }


        idt
    };
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct InterruptFrame {
    pub r15: usize,
    pub r14: usize,
    pub r13: usize,
    pub r12: usize,
    pub rbp: usize,
    pub rbx: usize,

    pub r11: usize,
    pub r10: usize,
    pub r9: usize,
    pub r8: usize,
    pub rsi: usize,
    pub rdi: usize,
    pub rdx: usize,
    pub rcx: usize,
    pub rax: usize,

    pub rip: usize,
    pub cs: usize,
    pub rflags: usize,
    pub rsp: usize,
    pub ss: usize,
}

impl InterruptFrame {
    #[inline(always)]
    pub fn is_user_mode(&self) -> bool {
        self.cs & 0x3 != 0
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct InterruptErrorFrame {
    pub code: usize,

    pub frame: InterruptFrame,
}

pub fn notify_eoi(index: u8) {
    unsafe { PICS.lock().notify_end_of_interrupt(index) }
}

macro_rules! interrupt_handler {
    ($name:ident, $num:literal, $push_error:expr) => {
        #[naked]
        unsafe extern "C" fn $name() {
            unsafe {
                core::arch::naked_asm!(concat!(
                    $push_error,
                    "
                    test qword ptr [rsp + 16], 0x3
                    jz 2f
                    swapgs
                2:
                    xchg [rsp], rax
                    ", crate::push_regs!(),"
                    push rax

                    mov rdi, {num}
                    mov rsi, rsp

                    call x64_handle_interrupt

                    add rsp, 8
                    ", crate::pop_regs!(),"

                    test qword ptr [rsp + 8], 0x3
                    jz 3f
                    swapgs
                3:
                    iretq
                    "
                ),
                num = const($num))
            }
        }
    };
}

macro_rules! has_error {
    () => {
        ""
    };
}
macro_rules! no_error {
    () => {
        "push 0"
    };
}
interrupt_handler!(divide_error_handler, 0x0, no_error!());
interrupt_handler!(debug_handler, 0x1, no_error!());
interrupt_handler!(nmi_handler, 0x2, no_error!());
interrupt_handler!(breakpoint_handler, 0x3, no_error!());
interrupt_handler!(overflow_handler, 0x4, no_error!());
interrupt_handler!(bound_range_exceeded_handler, 0x5, no_error!());
interrupt_handler!(invalid_opcode_handler, 0x6, no_error!());
interrupt_handler!(device_not_available_handler, 0x7, no_error!());

interrupt_handler!(double_fault_handler, 0x8, has_error!());
interrupt_handler!(invalid_tss_handler, 0xA, has_error!());
interrupt_handler!(segment_not_present_handler, 0xB, has_error!());
interrupt_handler!(stack_segment_fault_handler, 0xC, has_error!());
interrupt_handler!(general_protection_fault_handler, 0xD, has_error!());
interrupt_handler!(page_fault_handler, 0xE, has_error!());

interrupt_handler!(x87_floating_point_handler, 0x10, no_error!());

interrupt_handler!(alignment_check_handler, 0x11, has_error!());

interrupt_handler!(machine_check_handler, 0x12, no_error!());
interrupt_handler!(simd_floating_point_handler, 0x13, no_error!());
interrupt_handler!(virtualization_handler, 0x14, no_error!());

interrupt_handler!(vmm_communication_exception_handler, 0x1D, has_error!());
interrupt_handler!(security_exception_handler, 0x1E, has_error!());

interrupt_handler!(timer_handler, 32, no_error!());
interrupt_handler!(keyboard_handler, 33, no_error!());
interrupt_handler!(com2_handler, 35, no_error!());

use x86::irq::*;

#[no_mangle]
extern "C" fn x64_handle_interrupt(vector: u8, stack_frame: *mut InterruptErrorFrame) {
    let stack_frame = unsafe { &mut *stack_frame };
    let error_code = stack_frame.code;

    match vector {
        TIMER_IRQ => {
            super::time::pit_irq();
            let sched = get_scheduler();
            notify_eoi(TIMER_IRQ);
            sched.preempt();
        }
        KEYBOARD_IRQ => {
            do_keyboard_input();
            notify_eoi(KEYBOARD_IRQ);
        }
        COM2_IRQ => {
            notify_eoi(COM2_IRQ);
        }
        DIVIDE_ERROR_VECTOR => {
            log::error!("\nEXCEPTION: DIVIDE ERROR\n{:#x?}", stack_frame);
            panic!()
        }
        DEBUG_VECTOR => {
            log::error!("\nEXCEPTION: DEBUG EXCEPTION\n{:#x?}", stack_frame);
        }
        NONMASKABLE_INTERRUPT_VECTOR => {
            log::error!("\nEXCEPTION: NON-MASKABLE INTERRUPT\n{:#x?}", stack_frame);
            panic!()
        }
        OVERFLOW_VECTOR => {
            log::error!("\nEXCEPTION: OVERFLOW\n{:#x?}", stack_frame);
            panic!()
        }
        BOUND_RANGE_EXCEEDED_VECTOR => {
            log::error!("\nEXCEPTION: BOUND RANGE EXCEEDED\n{:#x?}", stack_frame);
            panic!()
        }
        INVALID_OPCODE_VECTOR => {
            log::error!("\nEXCEPTION: INVALID OPCODE\n{:#x?}", stack_frame);
            panic!()
        }
        DEVICE_NOT_AVAILABLE_VECTOR => {
            log::error!("\nEXCEPTION: DEVICE NOT AVAILABLE\n{:#x?}", stack_frame);
            panic!()
        }
        DOUBLE_FAULT_VECTOR => {
            log::error!(
                "\nEXCEPTION: DOUBLE FAULT\n{:#x?}\nError code: {:#b}\ncr3: {:#x}",
                stack_frame,
                error_code,
                Cr3::read_raw().0.start_address().as_u64()
            );
            panic!()
        }
        INVALID_TSS_VECTOR => {
            log::error!(
                "\nEXCEPTION: INVALID TSS\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            panic!()
        }
        SEGMENT_NOT_PRESENT_VECTOR => {
            log::error!(
                "\nEXCEPTION: SEGMENT NOT PRESENT\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            panic!()
        }
        STACK_SEGEMENT_FAULT_VECTOR => {
            log::error!(
                "\nEXCEPTION: STACK SEGMENT FAULT\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            panic!()
        }
        GENERAL_PROTECTION_FAULT_VECTOR => {
            log::error!(
                "\nEXCEPTION: GENERAL PROTECTION FAULT\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            unsafe {
                let fsbase = rdmsr(IA32_FS_BASE);
                // let gsbase = rdmsr(IA32_GS_BASE);
                log::debug!("FSBASE: {:#x}", fsbase);
            }
            if stack_frame.frame.is_user_mode() {
                backtrace::unwind_user_stack_from(stack_frame.frame.rbp, stack_frame.frame.rip);
                get_scheduler().exit_current(1);
            }
            panic!()
        }
        PAGE_FAULT_VECTOR => {
            let accessed_address = x86_64::registers::control::Cr2::read_raw();
            let cr3 = x86_64::registers::control::Cr3::read_raw().0;
            let error_code = PageFaultErrorCode::from_bits_truncate(error_code as u64);
            let current = get_scheduler().current_task_opt();
            if let Some(current) = current {
                if current
                    .handle_page_fault(
                        VirtAddr::new(accessed_address as usize),
                        *stack_frame,
                        error_code,
                    )
                    .is_err()
                {
                    log::error!(
                        "\nEXCEPTION: USER PAGE FAULT while accessing {:#x}\n\
                        error code: {:?}\ncr3: {:#x}\n{:#x?}",
                        accessed_address,
                        error_code,
                        cr3.start_address().as_u64(),
                        stack_frame,
                    );
                    let rip = stack_frame.frame.rip;
                    log::error!("Exception IP {:#x}", rip);
                    log::error!("Faulted access address {:#x}", accessed_address,);
                    panic!()
                }
            } else {
                log::error!(
                    "\nEXCEPTION: KERNEL PAGE FAULT while accessing {:#x}\n\
                    error code: {:?}\ncr3: {:#x}\n{:#x?}",
                    accessed_address,
                    error_code,
                    cr3.start_address().as_u64(),
                    stack_frame,
                );
                let rip = stack_frame.frame.rip;
                log::error!("Exception IP {:#x}", rip);
                log::error!("Faulted access address {:#x}", accessed_address,);
                panic!()
            }
        }
        X87_FPU_VECTOR => {
            log::error!("\nEXCEPTION: x87 FLOATING POINT\n{:#x?}", stack_frame);
            panic!()
        }
        ALIGNMENT_CHECK_VECTOR => {
            log::error!(
                "\nEXCEPTION: ALIGNMENT CHECK\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            panic!()
        }
        MACHINE_CHECK_VECTOR => {
            log::error!("\nEXCEPTION: MACHINE CHECK\n{:#x?}", stack_frame);
            panic!()
        }
        SIMD_FLOATING_POINT_VECTOR => {
            log::error!("\nEXCEPTION: SIMD FLOATING POINT\n{:#x?}", stack_frame);
            panic!()
        }
        VIRTUALIZATION_VECTOR => {
            log::error!("\nEXCEPTION: VIRTUALIZATION\n{:#x?}", stack_frame);
            panic!()
        }
        0x1D => {
            log::error!(
                "\nEXCEPTION: VMM COMMUNICATION EXCEPTION\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            panic!()
        }
        0x1E => {
            log::error!(
                "\nEXCEPTION: SECURITY EXCEPTION\n{:#x?}\nError code: {:#b}",
                stack_frame,
                error_code
            );
            panic!()
        }
        _ => log::warn!("Unhandled interrupt: {}", vector),
    }
}

pub const PIC_1_DATA_PORT: u8 = 0x21;
pub const PIC_2_DATA_PORT: u8 = 0xa1;

pub fn mask_irq(irq: u8) {
    let irq = irq - PIC_1_OFFSET;
    let port = if irq < 8 {
        PIC_1_DATA_PORT
    } else {
        PIC_2_DATA_PORT
    };
    let irq = if irq < 8 { irq } else { irq - 8 };
    let val = unsafe { x86::io::inb(port as u16) } | (1 << irq);
    unsafe { outb(port as u16, val) };
}

pub fn unmask_irq(irq: u8) {
    let irq = irq - PIC_1_OFFSET;
    let port = if irq < 8 {
        PIC_1_DATA_PORT
    } else {
        PIC_2_DATA_PORT
    };
    let irq = if irq < 8 { irq } else { irq - 8 };
    let val = unsafe { x86::io::inb(port as u16) } & !(1 << irq);
    unsafe { outb(port as u16, val) };
}

pub fn init() {
    IDT.load();
    unsafe {
        PICS.lock().initialize();
    }

    unmask_irq(TIMER_IRQ);
    unmask_irq(KEYBOARD_IRQ);
    unmask_irq(COM2_IRQ);
}

fn do_keyboard_input() {
    lazy_static! {
        static ref KEYBOARD: IrqMutex<Keyboard<Us104Key, ScancodeSet1>> = IrqMutex::new(
            Keyboard::new(ScancodeSet1::new(), Us104Key, HandleControl::Ignore)
        );
    }

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    let mut keyboard = KEYBOARD.lock();
    if let Ok(Some(key_evt)) = keyboard.add_byte(scancode) {
        if let Some(kbd) = KBD_DEVICE.get() {
            kbd.handle_kbd_irq(&key_evt);
        }
        if let Some(key) = keyboard.process_keyevent(key_evt) {
            match key {
                DecodedKey::Unicode(c) => {
                    TTY.get().unwrap().input_char(c as u8);
                }
                DecodedKey::RawKey(_code) => {}
            }
        }
    }
}
