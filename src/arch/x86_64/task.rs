#[derive(Clone, Debug, Default)]
#[repr(C)]
pub struct Context {
    cr3: usize,

    r15: usize,
    r14: usize,
    r13: usize,
    r12: usize,

    rbx: usize,
    rbp: usize,

    rip: usize,
}
use core::alloc::Layout;

use alloc::{alloc::alloc_zeroed, boxed::Box};
use x86::{
    msr::{wrmsr, IA32_FS_BASE, IA32_GS_BASE, rdmsr},
    segmentation::SegmentSelector,
    Ring, controlregs,
};

use crate::{
    mem::{
        addr::VirtAddr,
        addr_space::AddressSpace,
        consts::{KERNEL_STACK_SIZE, PAGE_SIZE},
    },
    util::{stack::Stack},
};

pub fn arch_context_switch(prev: &mut ArchTask, next: &mut ArchTask) {
    unsafe {
        // kpcr().tss.privilege_stack_table[0] = x86_64::VirtAddr::new((next.kernel_stack.as_ptr() as usize + next.kernel_stack.len()) as u64);

        prev.fsbase = VirtAddr::new(rdmsr(IA32_FS_BASE) as usize);
        prev.gsbase = VirtAddr::new(rdmsr(IA32_GS_BASE) as usize);
        wrmsr(IA32_FS_BASE, next.fsbase.value() as u64);
        wrmsr(IA32_GS_BASE, next.gsbase.value() as u64);

        context_switch(prev.context.as_mut(), next.context.as_mut())
    }
}

#[naked]
unsafe extern "C" fn iretq_init() -> ! {
    core::arch::asm!(
        "
    cli

    add rsp, 8

    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    
    pop r11
    pop r10
    pop r9
    pop r8
    pop rsi
    pop rdi
    pop rdx
    pop rcx
    pop rax

    iretq
    ",
        options(noreturn)
    )
}

#[naked]
unsafe extern "C" fn fork_init() -> ! {
    core::arch::asm!(
        "
        cli

        add rsp, 8

        pop r15
        pop r14
        pop r13
        pop r12
        pop rbp
        pop rbx
        
        pop r11
        pop r10
        pop r9
        pop r8
        pop rsi
        pop rdi
        pop rdx
        pop rcx
        pop rax

        swapgs
        iretq
    ",
        options(noreturn)
    )
}

use super::{
    gdt::{KERNEL_CS_IDX, KERNEL_DS_IDX},
    idt::InterruptFrame, cpu_local::kpcr,
};
#[naked]
unsafe extern "sysv64" fn context_switch(_prev: &mut Context, _next: &mut Context) {
    core::arch::asm!(
        "
        push rbp
        push rbx
        push r12
        push r13
        push r14
        push r15
        
        mov rax, cr3
        push rax

        mov [rdi], rsp
        mov rsp, rsi

        pop rax
        mov cr3, rax

        pop r15
        pop r14
        pop r13
        pop r12
        pop rbx
        pop rbp

        ret
    ",
        options(noreturn)
    )
}

#[naked]
unsafe extern "C" fn usermode_entry() -> ! {
    core::arch::asm!(
        "
        push rdi
        push rsi
        push rdx

        cli

        pop r11
        pop rcx
        pop rsp

        swapgs
        sysretq
    ",
        options(noreturn)
    )
}

#[repr(C)]
pub struct ArchTask {
    context: core::ptr::Unique<Context>,
    // pub(super) rsp: u64,
    // pub(super) fsbase: AtomicCell<u64>,
    kernel_stack: Box<[u8]>,
    user: bool,
    address_space: AddressSpace,
    fsbase: VirtAddr,
    gsbase: VirtAddr,
}

unsafe impl Sync for ArchTask {}

impl ArchTask {
    pub fn new_idle() -> ArchTask {
        ArchTask {
            context: unsafe { core::ptr::Unique::new_unchecked(&mut Context {
                cr3: controlregs::cr3() as usize,
                ..Default::default()
            }) },
            address_space: AddressSpace::current(),
            kernel_stack: alloc::vec![0u8; PAGE_SIZE].into_boxed_slice(),
            user: false,
            fsbase: VirtAddr::null(),
            gsbase: VirtAddr::null(),
        }
    }

    pub fn new_kernel(entry_point: VirtAddr, enable_interrupts: bool) -> ArchTask {
        let kernel_stack = alloc::vec![0u8; PAGE_SIZE].into_boxed_slice();
        let task_stack = unsafe {
            alloc_zeroed(Layout::from_size_align_unchecked(
                KERNEL_STACK_SIZE,
                KERNEL_STACK_SIZE,
            ))
            .add(KERNEL_STACK_SIZE)
        };

        let address_space = AddressSpace::current();

        let mut stack_ptr = kernel_stack.as_ptr() as usize;
        let mut stack = Stack::new(&mut stack_ptr);

        let kframe = unsafe { stack.offset::<InterruptFrame>() };
        kframe.ss = SegmentSelector::new(KERNEL_DS_IDX, Ring::Ring0).bits() as u64;
        kframe.cs = SegmentSelector::new(KERNEL_CS_IDX, Ring::Ring0).bits() as u64;
        kframe.rip = entry_point.value() as u64;
        kframe.rsp = task_stack as u64;
        kframe.rflags = if enable_interrupts { 0x200 } else { 0 };

        let context = unsafe { stack.offset::<Context>() };
        *context = Context::default();
        context.rip = iretq_init as usize;
        context.cr3 = unsafe { controlregs::cr3() as usize };
        Self {
            context: unsafe { core::ptr::Unique::new_unchecked(context) },
            address_space,
            kernel_stack,
            user: false,
            fsbase: VirtAddr::null(),
            gsbase: VirtAddr::null(),
        }
    }
}
