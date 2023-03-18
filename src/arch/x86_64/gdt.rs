use core::alloc::Layout;

use alloc::alloc::alloc_zeroed;

use x86::{
    msr::{wrmsr, IA32_GS_BASE},
    segmentation::{load_cs, load_ds, load_es, load_ss, SegmentSelector},
    task::load_tr,
    Ring,
};
use x86_64::structures::{
    gdt::{Descriptor, GlobalDescriptorTable},
    tss::TaskStateSegment,
};

use crate::mem::consts::KERNEL_STACK_SIZE;

use super::cpu_local::{kpcr, Kpcr};

pub const KERNEL_CS_IDX: u16 = 1;
pub const KERNEL_DS_IDX: u16 = 2;
pub const USER_CS_IDX: u16 = 3;
pub const USER_DS_IDX: u16 = 4;
const TSS_IDX: u16 = 5;

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
