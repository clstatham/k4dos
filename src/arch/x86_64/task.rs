use core::{alloc::Layout, slice::SlicePattern};

use alloc::{alloc::alloc_zeroed, boxed::Box, vec::Vec};
use x86::{
    cpuid::CpuId,
    current::segmentation::swapgs,
    msr::{rdmsr, wrmsr, IA32_FS_BASE, IA32_GS_BASE},
    tlb,
};
use x86_64::instructions::interrupts;

use crate::{
    fs::FileRef,
    mem::{
        addr::VirtAddr,
        addr_space::AddressSpace,
        allocator::alloc_kernel_frames,
        consts::{KERNEL_STACK_SIZE, PAGE_SIZE, USER_STACK_BOTTOM, USER_STACK_TOP},
    },
    task::{
        signal::Signal,
        vmem::{MMapFlags, MMapKind, MMapProt, Vmem},
    },
    userland::elf::{self, AuxvType},
    util::{stack::Stack, KResult},
};

use super::{
    cpu_local::get_tss,
    gdt::{KERNEL_CS_IDX, KERNEL_DS_IDX, USER_DS_IDX},
    idt::{InterruptFrame, InterruptErrorFrame},
};

fn xsave(fpu: &mut Box<[u8]>) {
    unsafe {
        core::arch::asm!("xsave [{}]", in(reg) fpu.as_ptr(), in("eax") 0xffffffffu32, in("edx") 0xffffffffu32)
    }
}

fn xrstor(fpu: &mut Box<[u8]>) {
    unsafe {
        core::arch::asm!("xrstor [{}]", in(reg) fpu.as_ptr(), in("eax") 0xffffffffu32, in("edx") 0xffffffffu32);
    }
}

pub fn arch_context_switch(prev: &mut ArchTask, next: &mut ArchTask) {
    unsafe {
        prev.fsbase = VirtAddr::new(rdmsr(IA32_FS_BASE) as usize);
        prev.gsbase = VirtAddr::new(rdmsr(IA32_GS_BASE) as usize);
        wrmsr(IA32_FS_BASE, next.fsbase.value() as u64);
        swapgs();
        wrmsr(IA32_GS_BASE, next.gsbase.value() as u64);
        get_tss().privilege_stack_table[0] = x86_64::VirtAddr::new(
            (next.kernel_stack.as_ptr() as usize + next.kernel_stack.len()) as u64,
        );
        swapgs();

        if let Some(fpu) = prev.fpu_storage.as_mut() {
            xsave(fpu);
        }

        if let Some(fpu) = next.fpu_storage.as_mut() {
            xrstor(fpu)
        }

        // log::debug!("Next context: {:#x?}", *next.context.as_ref());

        next.address_space.switch();
        // interrupts::disable(); // why doesn't this work instead of the FIXME in fork()?
        context_switch(&mut prev.context, next.context.as_ref())
    }
    // unreachable!("context_switch returned?");
}

#[naked]
unsafe extern "C" fn iretq_init() -> ! {
    core::arch::asm!(
        "
    cli
    
    add rsp, 8
    ",
        crate::pop_regs!(),
        "
    
    iretq
    ",
        options(noreturn)
    )
}

#[naked]
unsafe extern "C" fn fork_init() -> ! {
    core::arch::asm!(
        concat!(
            "
        cli
        
        add rsp, 8
        ",
            crate::pop_regs!(),
            "

        swapgs
        iretq
    "
        ),
        options(noreturn)
    )
}

#[naked]
unsafe extern "C" fn context_switch(_prev: &mut core::ptr::Unique<Context>, _next: &Context) {
    core::arch::asm!("
        pushfq
        push rbp
        push rbx
        push r12
        push r13
        push r14
        push r15

        mov [rdi], rsp
        mov rsp, rsi

        pop r15
        pop r14
        pop r13
        pop r12
        pop rbx
        pop rbp
        popfq

        ret

    ", options(noreturn))
}
// use memoffset::offset_of;
// #[naked]
// unsafe extern "sysv64" fn context_switch(_prev: &mut Context, _next: &mut Context) {
//     core::arch::asm!("\
//         mov [rdi + {off_rbx}], rbx
//         mov rbx, [rsi + {off_rbx}]

//         mov [rdi + {off_r12}], r12
//         mov r12, [rsi + {off_r12}]

//         mov [rdi + {off_r13}], r13
//         mov r13, [rsi + {off_r13}]

//         mov [rdi + {off_r14}], r14
//         mov r14, [rsi + {off_r14}]

//         mov [rdi + {off_r15}], r15
//         mov r15, [rsi + {off_r15}]

//         mov [rdi + {off_rbp}], rbp
//         mov rbp, [rsi + {off_rbp}]

//         mov [rdi + {off_rsp}], rsp
//         mov rsp, [rsi + {off_rsp}]

//         pushfq
//         pop qword ptr [rdi + {off_rflags}]

//         push qword ptr [rsi + {off_rflags}]
//         popfq
        
//         jmp {hook}
//     ", 
//     // pushfq
//         // pop qword ptr [rdi + {off_rflags}]

//         // push qword ptr [rsi + {off_rflags}]
//         // popfq
//     off_rflags = const(offset_of!(Context, rflags)),
//     // off_rip = const(offset_of!(Context, rip)),
//     off_rbx = const(offset_of!(Context, rbx)),
//     off_r12 = const(offset_of!(Context, r12)),
//     off_r13 = const(offset_of!(Context, r13)),
//     off_r14 = const(offset_of!(Context, r14)),
//     off_r15 = const(offset_of!(Context, r15)),
//     off_rbp = const(offset_of!(Context, rbp)),
//     off_rsp = const(offset_of!(Context, rsp)),
//     hook = sym switch_finish_hook,
//     options(noreturn))
// }


#[derive(Clone, Debug, Default)]
#[repr(C)]
pub struct Context {
    r15: usize,
    r14: usize,
    r13: usize,
    r12: usize,

    rbx: usize,
    rbp: usize,

    rflags: usize,
    rip: usize,
}

#[naked]
unsafe extern "C" fn exec_entry(rcx: usize, rsp: usize, r11: usize) -> ! {
    unsafe {
        core::arch::asm!(
            "
            cli
            swapgs

            mov r11, rdx
            mov rcx, rdi
            mov rsp, rsi

            mov r15, {user_ds}
            mov ds, r15d
            mov es, r15d
            mov fs, r15d
            mov gs, r15d

            xor rax, rax
            xor rbx, rbx
            xor rdx, rdx
            xor rsi, rsi
            xor rbp, rbp
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
}

#[repr(C)]
pub struct ArchTask {
    context: core::ptr::Unique<Context>,
    kernel_stack: Box<[u8]>,
    user: bool,
    pub(crate) address_space: AddressSpace,
    fsbase: VirtAddr,
    gsbase: VirtAddr,
    fpu_storage: Option<Box<[u8]>>,
}

unsafe impl Sync for ArchTask {}

impl ArchTask {
    pub fn new_idle() -> ArchTask {
        ArchTask {
            context: unsafe {
                core::ptr::Unique::new_unchecked(&mut Context {
                    // cr3: controlregs::cr3() as usize,
                    ..Default::default()
                })
            },
            address_space: AddressSpace::current(),
            kernel_stack: alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice(),
            user: false,
            fsbase: VirtAddr::null(),
            gsbase: VirtAddr::null(),
            fpu_storage: None,
        }
    }

    pub fn new_kernel(entry_point: VirtAddr, enable_interrupts: bool) -> ArchTask {
        // let switch_stack = alloc::vec![0u8; PAGE_SIZE].into_boxed_slice();
        let switch_stack = Self::alloc_switch_stack().unwrap();
        let task_stack = unsafe {
            alloc_zeroed(Layout::from_size_align_unchecked(
                KERNEL_STACK_SIZE,
                PAGE_SIZE,
            ))
            .add(KERNEL_STACK_SIZE)
        };

        let address_space = AddressSpace::current();

        let mut stack_ptr = switch_stack.as_mut_ptr::<u8>() as usize;
        let mut stack = Stack::new(&mut stack_ptr);

        let kframe = unsafe { stack.offset::<InterruptErrorFrame>() };
        *kframe = InterruptErrorFrame::default();
        kframe.frame.ss = (KERNEL_DS_IDX as usize) << 3;
        kframe.frame.cs = (KERNEL_CS_IDX as usize) << 3;
        kframe.frame.rip = entry_point.value();
        kframe.frame.rsp = task_stack as usize;
        kframe.frame.rflags = if enable_interrupts { 0x200 } else { 0 };

        // unsafe { stack.push(iretq_init as usize) };
        // let kframe_rsp = stack.top();

        let context = unsafe { stack.offset::<Context>() };
        *context = Context::default();
        // context.rip = iretq_init as usize;
        context.rip = iretq_init as usize;
        // context.rflags = kframe.frame.rflags;
        // context.cr3 = unsafe { controlregs::cr3() as usize };
        Self {
            context: unsafe { core::ptr::Unique::new_unchecked(context) },
            address_space,
            kernel_stack: alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice(),
            user: false,
            fsbase: unsafe { VirtAddr::new(rdmsr(IA32_FS_BASE) as usize) },
            gsbase: unsafe { VirtAddr::new(rdmsr(IA32_GS_BASE) as usize) },
            // gsbase: VirtAddr::null(),
            fpu_storage: None,
        }
    }

    pub fn exec(
        &mut self,
        vmem: &mut Vmem,
        file: FileRef,
        argv: &[&[u8]],
        envp: &[&[u8]],
    ) -> KResult<()> {
        interrupts::disable();
        let userland_entry = elf::load_elf(file)?;

        // let switch_stack = alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        // let switch_stack = Self::alloc_switch_stack().unwrap();

        self.kernel_stack = alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        self.fsbase = userland_entry.fsbase.unwrap_or(VirtAddr::null());
        self.gsbase = unsafe { VirtAddr::new(rdmsr(IA32_GS_BASE) as usize) };

        self.user = true;
        self.address_space = userland_entry.addr_space;

        *vmem = userland_entry.vmem;

        self.address_space.switch();

        // userland_entry
        vmem.map_area(
            VirtAddr::new(USER_STACK_BOTTOM),
            VirtAddr::new(USER_STACK_TOP),
            MMapFlags::empty(),
            MMapProt::PROT_READ | MMapProt::PROT_WRITE | MMapProt::PROT_EXEC,
            MMapKind::Anonymous,
            &mut self.address_space.mapper(),
        )?;

        // first the kernel stack for the context switch

        // let mut stack_ptr = switch_stack.as_mut_ptr::<u8>() as usize;
        // let mut stack = Stack::new(&mut stack_ptr);

        // let kframe = unsafe { stack.offset::<UserlandEntryRegs>() };
        // // *kframe = InterruptFrame::default();
        // // kframe.ss = (USER_DS_IDX as u64) << 3 | 3;
        // // kframe.cs = (USER_CS_IDX as u64) << 3 | 3;
        // // kframe.rip = userland_entry.entry_point.value() as u64;
        // kframe.rcx = userland_entry.entry_point.value();
        // kframe.r11 = 0x200;

        // let context = unsafe { stack.offset::<Context>() };
        // *context = Context::default();
        // let mut context = Context::default();
        
        // context.rflags = 0x200;

        let mut stack_addr = USER_STACK_TOP - core::mem::size_of::<usize>();
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
            // stack.push(envp_tops.as_slice());
            for envp_top in envp_tops.iter() {
                stack.push(*envp_top);
            }
            stack.push(0u64);
            // stack.push(argv_tops.as_slice());
            for argv_top in argv_tops.iter() {
                stack.push(*argv_top);
            }
            stack.push(argv_tops.len());
        }

        core::mem::drop(argv_tops);
        core::mem::drop(envp_tops);
        assert_eq!(stack.top() % 16, 0);

        // kframe.rsp = stack.top();
        self.fpu_storage = Some(Self::alloc_fpu_storage());
        // context.rip = userland_entry.entry_point.value();
        self.context = core::ptr::Unique::dangling();
        // unsafe {
        //     *self.context.as_mut() = context;
        // }
        unsafe {
            exec_entry(userland_entry.entry_point.value(), stack.top(), 0x200);
        }
        // unreachable!();
        // Ok(())
    }

    pub fn fork(&self) -> KResult<Self> {
        assert!(self.user, "Cannot fork a kernel task");

        let address_space = AddressSpace::current().fork(true)?;
        unsafe { tlb::flush_all() };

        let switch_stack = Self::alloc_switch_stack()?.as_mut_ptr::<u8>();
        let mut old_rsp = self.kernel_stack.as_ptr() as usize + self.kernel_stack.len();
        let mut old_stack = Stack::new(&mut old_rsp);

        let mut new_rsp = switch_stack as usize;
        let mut new_stack = Stack::new(&mut new_rsp);

        unsafe {
            let new_frame = new_stack.offset::<InterruptErrorFrame>();
            let old_frame = old_stack.offset::<InterruptErrorFrame>();
            // log::debug!("Old frame: {:#x?}", syscall_frame);
            *new_frame = *old_frame;
            // new_frame.frame = *syscall_frame;
            // new_frame.cs = syscall_frame.cs;
            // new_frame.r10 = syscall_frame.r10;
            // new_frame.r11 = syscall_frame.r11;
            // new_frame.r12 = syscall_frame.r12;
            // new_frame.r13 = syscall_frame.r13;
            // new_frame.r14 = syscall_frame.r14;
            // new_frame.r15 = syscall_frame.r15;
            // new_frame.r8 = syscall_frame.r8;
            // new_frame.r9 = syscall_frame.r9;
            // new_frame.rbp = syscall_frame.rbp;
            // new_frame.rbx = syscall_frame.rbx;
            // new_frame.rcx = syscall_frame.rcx;
            // new_frame.rdi = syscall_frame.rdi;
            // new_frame.rsi = syscall_frame.rsi;
            // new_frame.ss = syscall_frame.ss;
            // new_frame.rsp = syscall_frame.rsp;
            // new_frame.rip = syscall_frame.rip;
            // new_frame.rflags = syscall_frame.rflags;

            new_frame.frame.rax = 0x0; // fork return value

            // // fixme: having interrupts enabled between the context switch and fork_init being called will clobber the stack
            new_frame.frame.rflags = old_frame.frame.rflags & !0x200;

            // new_stack.push(fork_init as usize);
        }
        // let kframe_rsp = new_stack.top();

        let context = unsafe { new_stack.offset::<Context>() };
        *context = Context::default();
        context.rip = fork_init as usize;
        let mut fpu_storage = Self::alloc_fpu_storage();
        fpu_storage.copy_from_slice(self.fpu_storage.as_ref().unwrap().as_slice());
        Ok(Self {
            context: unsafe { core::ptr::Unique::new_unchecked(context) },
            address_space,
            user: true,
            kernel_stack: alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice(),
            fsbase: self.fsbase,
            gsbase: self.gsbase,
            fpu_storage: Some(fpu_storage),
        })
    }

    pub fn clone_process(
        &self,
        entry_point: VirtAddr,
        user_stack: VirtAddr,
        args: VirtAddr,
        r8: usize,
        r9: usize,
        syscall_frame: &InterruptFrame,
    ) -> KResult<Self> {
        assert!(self.user, "Cannot clone a kernel task");

        let address_space = AddressSpace::current().fork(true)?;
        let switch_stack = Self::alloc_switch_stack()?.as_mut_ptr::<u8>();

        let mut new_rsp = switch_stack as usize;
        let mut new_stack = Stack::new(&mut new_rsp);

        let new_frame = unsafe { new_stack.offset::<InterruptErrorFrame>() };
        *new_frame = InterruptErrorFrame::default();
        // *new_frame = *syscall_frame;

        new_frame.frame.cs = syscall_frame.cs;
        new_frame.frame.ss = syscall_frame.ss;

        new_frame.frame.r8 = r8;
        new_frame.frame.r9 = r9;
        new_frame.frame.rdi = args.value();

        new_frame.frame.rip = entry_point.value();
        new_frame.frame.rsp = user_stack.value();
        new_frame.frame.rflags = 0x200;

        // unsafe { new_stack.push(fork_init as usize) };
        // let kframe_rsp = new_stack.top();
        let context = unsafe { new_stack.offset::<Context>() };
        *context = Context::default();
        // *context = unsafe { self.context.as_ref() }.clone();
        // context.rsp = kframe_rsp;
        context.rip = fork_init as usize;

        let mut fpu_storage = Self::alloc_fpu_storage();
        fpu_storage.copy_from_slice(self.fpu_storage.as_ref().unwrap().as_slice());

        Ok(Self {
            context: unsafe { core::ptr::Unique::new_unchecked(context) },
            address_space,
            user: true,
            fpu_storage: Some(fpu_storage),
            gsbase: self.gsbase,
            fsbase: self.fsbase,
            kernel_stack: alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice(),
        })
    }

    fn alloc_fpu_storage() -> Box<[u8]> {
        unsafe {
            let xsave_size = CpuId::new().get_extended_state_info().unwrap().xsave_size();
            let layout = Layout::from_size_align(xsave_size as usize, 8).unwrap();
            let ptr = alloc_zeroed(layout);
            Box::from_raw(core::ptr::slice_from_raw_parts_mut(
                ptr,
                xsave_size as usize,
            ))
        }
    }

    fn alloc_switch_stack() -> KResult<VirtAddr> {
        Ok(alloc_kernel_frames(1)?.start_address().as_hhdm_virt() + PAGE_SIZE)
    }

    pub fn set_fsbase(&mut self, addr: VirtAddr) {
        self.fsbase = addr;
        unsafe {
            wrmsr(IA32_FS_BASE, addr.value() as u64);
        }
    }

    pub fn setup_signal_stack(
        frame: &mut InterruptFrame,
        signal: Signal,
        handler: VirtAddr,
        _syscall_result: isize,
        // sigreturn: VirtAddr,
    ) -> KResult<()> {
        const TRAMPOLINE: &[u8] = &[
            0xb8, 0x0f, 0x00, 0x00, 0x00, // mov eax, 15
            0x0f, 0x05, // syscall
            0x90, // nop (for alignment)
        ];

        // let mut rsp = unsafe { self.context.as_ref().rsp as usize };
        if frame.cs & 0x3 == 0 {
            return Ok(());
        }
        let mut rsp = frame.rsp;
        let mut stack = Stack::new(&mut rsp);
        // red zone
        stack.skip_by(128);
        // let tramp_rip = stack.top();
        // log::debug!("{:#x?}", tramp_rip);
        unsafe {
            stack.push_bytes(TRAMPOLINE);
            // stack.push_bytes(TRAMPOLINE);
            // stack.push(0usize); // todo: sigreturn
            stack.push(stack.top());
        }

        frame.rip = handler.value();
        frame.rsp = rsp;
        frame.rdi = signal as usize;
        // frame.rsi = 0;
        // frame.rdx = 0;

        Ok(())
    }

    pub fn setup_sigreturn_stack(
        &self,
        current_frame: &mut InterruptFrame,
        signaled_frame: &InterruptFrame,
    ) {
        *current_frame = *signaled_frame;
    }
}
