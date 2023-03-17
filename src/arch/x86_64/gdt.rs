use core::alloc::Layout;

use alloc::alloc::alloc_zeroed;
use lazy_static::lazy_static;
use x86::{
    controlregs::{cr4_write, Cr4},
    current::segmentation::{rdgsbase, wrgsbase},
    segmentation::{load_cs, load_ds, load_es, load_ss, SegmentSelector},
    task::load_tr,
    Ring, msr::{wrmsr, IA32_GS_BASE, rdmsr},
};
use x86_64::structures::{
    gdt::{Descriptor, GlobalDescriptorTable},
    tss::TaskStateSegment, idt::InterruptDescriptorTable,
};

use crate::mem::consts::KERNEL_STACK_SIZE;

const KERNEL_CS_IDX: u16 = 1;
const KERNEL_DS_IDX: u16 = 2;
const USER_CS_IDX: u16 = 3;
const USER_DS_IDX: u16 = 4;
const TSS_IDX: u16 = 5;

#[repr(C)]
pub struct Kpcr {
    pub user_rsp_tmp: usize,
    pub kernel_sp: usize,
    pub tss: TaskStateSegment,
    pub gdt: GlobalDescriptorTable,
    pub idt: InterruptDescriptorTable,
}

pub fn kpcr() -> &'static mut Kpcr {
    unsafe { &mut *(rdmsr(IA32_GS_BASE) as *mut _) }
}

static STACK: [u8; KERNEL_STACK_SIZE] = [0; KERNEL_STACK_SIZE];

pub fn init() {
    unsafe {

        let kpcr_layout = Layout::new::<Kpcr>();
        let kpcr_ptr = alloc_zeroed(kpcr_layout) as *mut Kpcr;
        wrmsr(IA32_GS_BASE, kpcr_ptr as u64);
    }

    let mut tss = TaskStateSegment::new();
    tss.privilege_stack_table[0] = x86_64::VirtAddr::new(STACK.as_ptr() as u64);

    kpcr().tss = tss;

    let mut gdt = GlobalDescriptorTable::new();
    // kernel code
    gdt.add_entry(Descriptor::kernel_code_segment());
    // kernel data
    gdt.add_entry(Descriptor::kernel_data_segment());
    // // kernel tls
    // gdt.add_entry(Descriptor::kernel_data_segment());
    // user code
    gdt.add_entry(Descriptor::user_code_segment());
    // user data (syscall)
    gdt.add_entry(Descriptor::user_data_segment());
    // // user tls
    // gdt.add_entry(Descriptor::user_data_segment());
    // TSS
    gdt.add_entry(Descriptor::tss_segment(&kpcr().tss));

    kpcr().gdt = gdt;
    kpcr().gdt.load();

    unsafe {
        load_cs(SegmentSelector::new(KERNEL_CS_IDX, Ring::Ring0));
        load_ds(SegmentSelector::new(KERNEL_DS_IDX, Ring::Ring0));
        load_es(SegmentSelector::new(KERNEL_DS_IDX, Ring::Ring0));
        load_ss(SegmentSelector::new(KERNEL_DS_IDX, Ring::Ring0));

        load_tr(SegmentSelector::new(TSS_IDX, Ring::Ring0));
    }
}
