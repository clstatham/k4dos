use x86::msr::{wrmsr, rdmsr};
use x86::segmentation::SegmentSelector;
use x86::Ring;

use crate::userland::syscall::SyscallHandler;

use super::gdt::{KERNEL_CS_IDX, USER_CS_IDX};
use super::idt::InterruptFrame;

macro_rules! push_regs {
    () => {
        "   
        // push scratch regs
        push rcx
        push rdx
        push rdi
        push rsi
        push r8
        push r9
        push r10
        push r11

        // push preserved regs
        push rbx
        push rbp
        push r12
        push r13
        push r14
        push r15
        "
    };
}

macro_rules! pop_regs {
    () => {
        "


        // pop preserved regs
        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx

        // pop scratch regs
        pop r11
        pop r10
        pop r9
        pop r8
        pop rsi
        pop rdi
        pop rdx
        pop rcx
        pop rax
        "
    };
}

#[naked]
pub unsafe extern "C" fn syscall_entry() {
    use memoffset::offset_of;
    use x86_64::structures::tss::TaskStateSegment;
    core::arch::asm!(
        concat!(
            "
        cli
        swapgs
        mov gs:[{off} + {sp}], rsp
        mov rsp, gs:[{off} + {ksp}]
        push qword ptr {ss_sel}
        push qword ptr gs:[{off} + {sp}]
        push r11
        push qword ptr {cs_sel}
        push rcx

        push rax
        ",
            push_regs!(),
            "
        mov rdi, rsp
        call x64_handle_syscall
        ",
            pop_regs!(),
            "
        test dword ptr [rsp + 4], 0xFFFF8000
        jnz 1f

        pop rcx
        add rsp, 8
        pop r11
        pop qword ptr gs:[{off} + {sp}]
        mov rsp, gs:[{off} + {sp}]
        cli
        swapgs
        sysretq
    1:
        xor rcx, rcx
        xor r11, r11
        cli
        swapgs
        iretq
        "
        ),
        // off = const(0x1070), // todo: don't hardcode this
        off = const(0),
        // sp = const(offset_of!(TaskStateSegment, reserved_2)),
        sp = const(0x1c),
        ksp = const(offset_of!(TaskStateSegment, privilege_stack_table)),
        ss_sel = const(SegmentSelector::new(crate::arch::gdt::USER_DS_IDX, Ring::Ring3).bits()),
        cs_sel = const(SegmentSelector::new(crate::arch::gdt::USER_CS_IDX, Ring::Ring3).bits()),
        options(noreturn)
    )
}


#[no_mangle]
unsafe extern "C" fn x64_handle_syscall(ctx: *mut InterruptFrame) -> isize {
    let context = &*ctx;
    handle_syscall(
        context.rdi as usize,
        context.rsi as usize,
        context.rdx as usize,
        context.r10 as usize,
        context.r8 as usize,
        context.r9 as usize,
        context.rax as usize,
        ctx,
    )
}


#[allow(clippy::too_many_arguments)]
fn handle_syscall(
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    n: usize,
    frame: *mut InterruptFrame,
) -> isize {
    let mut handler = SyscallHandler {
        frame: unsafe { &mut *frame },
    };

    let retval = match handler.dispatch(a1, a2, a3, a4, a5, a6, n) {
        Ok(retval) => {
            // log::trace!("Syscall returned Ok");
            retval
        }
        Err(err) => {
            // if let Some(msg) = err.msg {
            log::error!(
                "Syscall handler returned Err {:?} with msg: {:?}",
                err.errno(),
                err.msg()
            );
            // }
            let errno = err.errno().unwrap() as i32;
            -errno as isize
        }
    };
    handler.frame.rax = retval as u64;
    retval
}

/// # Safety
/// This writes several MSR registers.
pub unsafe fn init() {
    let kernel_cs_offset = (KERNEL_CS_IDX as u64) << 3;
    let user_cs_offset = (USER_CS_IDX as u64) << 3;
    wrmsr(
        x86::msr::IA32_STAR,
        (user_cs_offset << 48) | (kernel_cs_offset << 32),
    );
    wrmsr(x86::msr::IA32_LSTAR, syscall_entry as *const u8 as u64);
    wrmsr(x86::msr::IA32_FMASK, 0x200);

    wrmsr(x86::msr::IA32_CSTAR, 0);

    wrmsr(x86::msr::IA32_EFER, rdmsr(x86::msr::IA32_EFER) | 1);
}
