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

use alloc::{alloc::alloc_zeroed, boxed::Box, vec::Vec};
use x86::{
    controlregs,
    msr::{rdmsr, wrmsr, IA32_FS_BASE, IA32_GS_BASE},
    segmentation::SegmentSelector,
    Ring, current::segmentation::swapgs,
};
use x86_64::structures::paging::PageTableFlags;

use crate::{
    mem::{
        addr::VirtAddr,
        addr_space::AddressSpace,
        consts::{KERNEL_STACK_SIZE, PAGE_SIZE, USER_STACK_BOTTOM, USER_STACK_TOP},
    },
    util::{stack::Stack, KResult}, task::vmem::Vmem, fs::FileRef, userland::elf::{ElfLoadError, self, AuxvType},
};

use super::{
    gdt::{KERNEL_CS_IDX, KERNEL_DS_IDX, USER_CS_IDX, USER_DS_IDX},
    idt::InterruptFrame, cpu_local::get_tss,
};

pub fn arch_context_switch(prev: &mut ArchTask, next: &mut ArchTask) {
    unsafe {
        get_tss().privilege_stack_table[0] = x86_64::VirtAddr::new((next.kernel_stack.as_ptr() as usize + next.kernel_stack.len()) as u64);

        prev.fsbase = VirtAddr::new(rdmsr(IA32_FS_BASE) as usize);
        prev.gsbase = VirtAddr::new(rdmsr(IA32_GS_BASE) as usize);
        wrmsr(IA32_FS_BASE, next.fsbase.value() as u64);
        // swapgs();
        wrmsr(IA32_GS_BASE, next.gsbase.value() as u64);
        // swapgs();
        
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
unsafe extern "C" fn usermode_entry(stack: VirtAddr, rip: VirtAddr, rflags: u64) -> ! {
    core::arch::asm!(
        "
        push rdi
        push rsi
        push rdx

        cli
        swapgs

        pop r11
        pop rcx
        pop rsp

        mov r15, {user_ds}
        mov ds, r15d
        mov es, r15d
        mov fs, r15d
        mov gs, r15d

        xor rax, rax
        xor rbx, rbx
        xor rdx, rdx
        xor rsi, rsi
        // xor rbp, rbp
        xor r8, r8
        xor r9, r9
        xor r10, r10
        xor r12, r12
        xor r13, r13
        xor r14, r14
        xor r15, r15
        
        sysretq
    ",
        user_ds = const((USER_DS_IDX as u64) << 3 | 3),
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
    pub(crate) address_space: AddressSpace,
    fsbase: VirtAddr,
    pub(crate) gsbase: VirtAddr,
}

unsafe impl Sync for ArchTask {}

impl ArchTask {
    pub fn new_idle() -> ArchTask {
        ArchTask {
            context: unsafe {
                core::ptr::Unique::new_unchecked(&mut Context {
                    cr3: controlregs::cr3() as usize,
                    ..Default::default()
                })
            },
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
            fsbase: unsafe { VirtAddr::new(rdmsr(IA32_FS_BASE) as usize) },
            gsbase: unsafe { VirtAddr::new(rdmsr(IA32_GS_BASE) as usize) },
        }
    }

    // #[allow(unreachable_code)]
    pub fn exec(&mut self, file: FileRef, argv: &[&[u8]], envp: &[&[u8]]) -> KResult<(), ElfLoadError> {
        let mut userland_entry = elf::load_elf(file, argv, envp)?;
        self.user = true;

        userland_entry.vmem.map_area(VirtAddr::new(USER_STACK_BOTTOM), VirtAddr::new(USER_STACK_TOP), PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::NO_EXECUTE, &mut userland_entry.addr_space.mapper()).map_err(|e| e.into())?;
        
        userland_entry.addr_space.switch();
        
        self.context = core::ptr::Unique::dangling();
        // self.context = core::ptr::Unique::new(&mut Context::default()).unwrap();
        self.address_space = userland_entry.addr_space;

        self.fsbase = userland_entry.fsbase.unwrap_or(VirtAddr::null());
        // self.gsbase = unsafe { VirtAddr::new(rdmsr(IA32_GS_BASE) as usize) };
        self.gsbase = VirtAddr::null();

        let mut stack_addr = USER_STACK_TOP;
        let mut stack = Stack::new(&mut stack_addr);

        fn push_strs(strs: &[&[u8]], stack: &mut Stack) -> Vec<usize> {
            let mut tops = Vec::new();
            for slice in strs.iter() {
                unsafe {
                    stack.push(0u8);
                    stack.push_bytes(slice);
                }
                tops.push(stack.top());
            }
            tops
        }

        let envp_tops = push_strs(envp, &mut stack);
        let argv_tops = push_strs(argv, &mut stack);

        stack.align_down(16);

        let size = envp.len() + 1 + argv.len() + 1 + 1;
        if size % 2 == 1 {
            unsafe {
                stack.push(0u64);
            }
        }

        unsafe {
            stack.push(0usize);
            stack.push(AuxvType::AtNull);
            stack.push(userland_entry.hdr);

            stack.push(0u64);
            stack.push(envp_tops.as_slice());
            stack.push(0u64);
            stack.push(argv_tops.as_slice());
            stack.push(argv_tops.len());
        }

        core::mem::drop(argv_tops);
        core::mem::drop(envp_tops);
        assert_eq!(stack.top() % 16, 0);

        unsafe {
            usermode_entry(VirtAddr::new(stack.top()), userland_entry.entry_point, 0x002);
        }

        // Ok(())
    }
}
