use x86::msr::{rdmsr, wrmsr};

use crate::userland::syscall::{errno_to_isize, SyscallHandler, QUIET_SYSCALLS};

use super::gdt::{KERNEL_CS_IDX, USER_DS_IDX};
use super::idt::InterruptFrame;

#[macro_export]
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

#[macro_export]
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
        cld
        call x64_handle_syscall
        cli
        ",
            pop_regs!(),
            "
        // test dword ptr [rsp + 4], 0xFFFF8000
        // jnz 1f

        pop rcx
        add rsp, 8
        pop r11
        // pop qword ptr gs:[{off} + {sp}]
        // mov rsp, gs:[{off} + {sp}]
        pop rsp
        cli
        swapgs
        sysretq
    // 1:
    //     xor rcx, rcx
    //     xor r11, r11
    //     cli
    //     swapgs
    //     iretq
        "
        ),
        off = const(0),
        sp = const(offset_of!(crate::arch::cpu_local::Kpcr, user_rsp0_tmp)),
        ksp = const(offset_of!(TaskStateSegment, privilege_stack_table)),
        ss_sel = const((crate::arch::gdt::USER_DS_IDX << 3) | 3),
        cs_sel = const((crate::arch::gdt::USER_CS_IDX << 3) | 3),
        options(noreturn)
    )
}

#[no_mangle]
unsafe extern "C" fn x64_handle_syscall(ctx: *mut InterruptFrame) -> isize {
    let context = &*ctx;
    handle_syscall(
        context.rdi,
        context.rsi,
        context.rdx,
        context.r10,
        context.r8,
        context.r9,
        context.rax,
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

    let res = handler.dispatch(a1, a2, a3, a4, a5, a6, n);
    if let Err(ref err) = res {
        if !QUIET_SYSCALLS.contains(&n) {
            log::error!(
                "Syscall handler returned Err {:?} with msg: {:?}",
                err.errno(),
                err.msg()
            );
        }
    }
    let retval = errno_to_isize(&res);
    handler.frame.rax = retval as usize;
    retval
}

/// # Safety
/// This writes several MSR registers.
pub unsafe fn init() {
    let kernel_cs_offset = (KERNEL_CS_IDX as u64) << 3;
    let user_ds_offset = (USER_DS_IDX as u64) << 3;
    let mut star = 0u64;
    star |= (user_ds_offset - 8) << 48;
    star |= kernel_cs_offset << 32;
    wrmsr(
        x86::msr::IA32_STAR,
        star,
    );
    wrmsr(x86::msr::IA32_LSTAR, syscall_entry as *const u8 as u64);
    wrmsr(x86::msr::IA32_FMASK, 0x200);

    wrmsr(x86::msr::IA32_CSTAR, 0);

    wrmsr(x86::msr::IA32_EFER, rdmsr(x86::msr::IA32_EFER) | 1);
}
