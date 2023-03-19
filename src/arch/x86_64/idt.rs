use lazy_static::lazy_static;
use pc_keyboard::{layouts::Us104Key, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

use pic8259::ChainedPics;
use spin::Mutex;
use x2apic::lapic::{xapic_base, LocalApic, LocalApicBuilder};
use x86::{
    current::segmentation::swapgs,
    io::outb,
    msr::{rdmsr, IA32_GS_BASE},
    segmentation::ss,
};
use x86_64::{
    instructions::port::Port,
    registers::control::Cr3,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
};

use crate::{
    mem::{addr::VirtAddr, consts::MAX_LOW_VADDR},
    task::get_scheduler,
    util::SpinLock,
};

use super::cpu_local::get_kpcr;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

// pub const TIMER_IDT_IDX: u8 = 32;
// pub const KEYBOARD_IDT_IDX: u8 = 33;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

pub const TIMER_IRQ: u8 = 32;
// pub const ERROR_IRQ: u8 = 34;
// pub const SPURIOUS_IRQ: u8 = 35;

lazy_static! {
    // pub static ref LAPIC: SpinLock<LocalApic> = SpinLock::new(
    //     LocalApicBuilder::new()
    //         .timer_vector(TIMER_IRQ as usize)
    //         .error_vector(ERROR_IRQ as usize)
    //         .spurious_vector(SPURIOUS_IRQ as usize)
    //         .set_xapic_base(unsafe { xapic_base() })
    //         .build()
    //         .unwrap()
    // );

    pub static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
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
            unsafe {
                idt.double_fault
                .set_handler_addr(x86_64::VirtAddr::new(double_fault_handler as u64))
                    .set_stack_index(0);
            }

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

            // idt[TIMER_IDT_IDX as usize].set_handler_fn(timer_handler);
            idt[TIMER_IRQ as usize].set_handler_addr(x86_64::VirtAddr::new(timer_handler as u64));
            // idt[ERROR_IRQ as usize].set_handler_fn(lapic_error_handler);
            // idt[SPURIOUS_IRQ as usize].set_handler_fn(lapic_spurious_handler);
        }


        idt
    };
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct InterruptFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,

    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,

    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

#[derive(Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct InterruptErrorFrame {
    pub code: u64,

    pub frame: InterruptFrame,
}

pub fn notify_eoi(index: u8) {
    unsafe { PICS.lock().notify_end_of_interrupt(index) }
    // unsafe { LAPIC.lock().end_of_interrupt() }
}

macro_rules! interrupt_handler {
    ($name:ident, $num:literal, $push_error:expr) => {
        #[naked]
        unsafe extern "C" fn $name() {
            core::arch::asm!(concat!(
                $push_error,
                "
                test qword ptr [rsp + 16], 0x3
                jz 1f
                swapgs
            1:
                xchg [rsp], rax
                ", crate::push_regs!(),"
                push rax

                mov rdi, {num}
                mov rsi, rsp

                call x64_handle_interrupt

                add rsp, 8
                ", crate::pop_regs!(),"

                test qword ptr [rsp + 8], 0x3
                jz 2f
                swapgs
            2:
                iretq
                "
            ),
            num = const($num),
            options(noreturn))
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

use x86::irq::*;

#[no_mangle]
extern "C" fn x64_handle_interrupt(vector: u8, stack_frame: *mut InterruptErrorFrame) {
    let stack_frame = unsafe { &mut *stack_frame };
    let error_code = stack_frame.code;

    match vector {
        TIMER_IRQ => {
            // log::info!("tick");
            if stack_frame.frame.cs & 0b11 != 0 {
                get_scheduler().with_kernel_addr_space_active(|| notify_eoi(TIMER_IRQ));
            } else {
                notify_eoi(TIMER_IRQ);
            }
        }
        // SPURIOUS_IRQ => {
        //     notify_eoi(0);
        // }
        // ERROR_IRQ => {
        //     panic!("LOCAL APIC ERROR: {:#x?}", stack_frame)
        // }
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
            panic!()
        }
        PAGE_FAULT_VECTOR => {
            let accessed_address = x86_64::registers::control::Cr2::read_raw();
            let cr3 = x86_64::registers::control::Cr3::read_raw().0;
            let error_code = PageFaultErrorCode::from_bits_truncate(error_code);
            if error_code.contains(PageFaultErrorCode::USER_MODE) {
                unsafe {
                    core::arch::asm!("swapgs");
                }
                get_scheduler()
                    .current_task()
                    .lock()
                    .as_ref()
                    .unwrap()
                    .handle_page_fault(
                        VirtAddr::new(accessed_address as usize),
                        VirtAddr::new(stack_frame.frame.rip as usize),
                        error_code,
                    );
                unsafe {
                    core::arch::asm!("swapgs");
                }
                return;
            }

            log::error!(
                "\nEXCEPTION: PAGE FAULT while accessing {:#x}\n\
                error code: {:?}\ncr3: {:#x}\n{:#x?}",
                accessed_address,
                error_code,
                cr3.start_address().as_u64(),
                stack_frame,
            );

            log::error!("Exception IP {:#x}", stack_frame.frame.rip as usize);
            log::error!("Faulted access address {:#x}", accessed_address,);
            panic!()
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

const DIVISOR: u16 = (1193182u32 / 1000) as u16;

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
        // let mut lock = LAPIC.lock();
        // lock.enable();
        PICS.lock().initialize();
    }
    log::info!("enabling PIT (i8254) timer: divisor={}", DIVISOR);
    unsafe {
        outb(0x43, 0x35);
        outb(0x40, (DIVISOR & 0xff) as u8);
        outb(0x40, (DIVISOR >> 8) as u8);
    }
    unmask_irq(TIMER_IRQ);
}

extern "x86-interrupt" fn lapic_error_handler(stack_frame: InterruptStackFrame) {
    panic!("LOCAL APIC ERROR: {:#x?}", stack_frame)
}

extern "x86-interrupt" fn lapic_spurious_handler(_stack_frame: InterruptStackFrame) {
    notify_eoi(0);
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

    notify_eoi(0);
}
