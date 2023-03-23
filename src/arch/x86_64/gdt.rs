use core::alloc::Layout;

use alloc::alloc::alloc_zeroed;

use lazy_static::lazy_static;
use x86::msr::{wrmsr, IA32_GS_BASE};
use x86_64::{
    instructions::tables::load_tss,
    registers::segmentation::{Segment, CS, DS, ES, FS, GS, SS},
    structures::{
        gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector},
        tss::TaskStateSegment,
    },
};

use crate::mem::consts::KERNEL_STACK_SIZE;

use super::cpu_local::{get_kpcr, get_tss, CpuLocalData, Kpcr};

pub const KERNEL_CS_IDX: u16 = 1;
pub const KERNEL_DS_IDX: u16 = 2;
const TSS_IDX: u16 = 3;
pub const USER_DS_IDX: u16 = 5;
pub const USER_CS_IDX: u16 = 6;

static mut STACK: [u8; KERNEL_STACK_SIZE] = [0; KERNEL_STACK_SIZE];

lazy_static! {
    static ref BOOT_GDT: (GlobalDescriptorTable, [SegmentSelector; 2]) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code_sel = gdt.add_entry(Descriptor::kernel_code_segment());
        let kernel_data_sel = gdt.add_entry(Descriptor::kernel_data_segment());
        // let kernel_tls_sel = gdt.add_entry(Descriptor::kernel_data_segment());
        (gdt, [kernel_code_sel, kernel_data_sel])
    };
}

pub fn init_boot() {
    unsafe {
        BOOT_GDT.0.load();
        CS::set_reg(BOOT_GDT.1[0]);
        DS::set_reg(BOOT_GDT.1[1]);
        ES::set_reg(BOOT_GDT.1[1]);
        FS::set_reg(BOOT_GDT.1[1]);

        GS::set_reg(BOOT_GDT.1[1]);

        SS::set_reg(BOOT_GDT.1[1]);
    }
}

pub fn init() {
    unsafe {
        let kpcr_layout = Layout::new::<Kpcr>();
        let kpcr_ptr = alloc_zeroed(kpcr_layout) as *mut Kpcr;
        wrmsr(IA32_GS_BASE, kpcr_ptr as u64);

        let tls_layout = Layout::new::<CpuLocalData>();
        let tls_ptr = alloc_zeroed(tls_layout) as *mut CpuLocalData;
        get_kpcr().cpu_local = &mut *tls_ptr;
    }

    let mut tss = get_tss();
    *tss = TaskStateSegment::new();
    tss.privilege_stack_table[0] =
        x86_64::VirtAddr::new(unsafe { STACK.as_ptr() } as u64 + KERNEL_STACK_SIZE as u64);

    let gdt = &mut get_kpcr().cpu_local.gdt;
    *gdt = GlobalDescriptorTable::new();
    // kernel code
    let kernel_cs_sel = gdt.add_entry(Descriptor::kernel_code_segment());
    // kernel data
    let kernel_ds_sel = gdt.add_entry(Descriptor::kernel_data_segment());
    // // kernel tls
    // let kernel_tls_sel = gdt.add_entry(Descriptor::kernel_data_segment());
    // TSS
    let tss_sel = gdt.add_entry(Descriptor::tss_segment(tss));
    // user data (syscall)
    let user_ds_sel = gdt.add_entry(Descriptor::user_data_segment());
    // user code
    let user_cs_sel = gdt.add_entry(Descriptor::user_code_segment());

    // // user tls
    // let user_tls_sel = gdt.add_entry(Descriptor::user_data_segment());

    // log::debug!("kernel_cs: ({:#x}) {:?}", kernel_cs_sel.0, kernel_cs_sel);
    // log::debug!("kernel_ds: ({:#x}) {:?}", kernel_ds_sel.0, kernel_ds_sel);
    // log::debug!("tss:       ({:#x}) {:?}", tss_sel.0, tss_sel);
    // log::debug!("user_cs:   ({:#x}) {:?}", user_cs_sel.0, user_cs_sel);
    // log::debug!("user_ds:   ({:#x}) {:?}", user_ds_sel.0, user_ds_sel);

    // get_kpcr().cpu_local.gdt = gdt;
    // get_kpcr().cpu_local.gdt.load();
    gdt.load();

    unsafe {
        CS::set_reg(kernel_cs_sel);
        DS::set_reg(kernel_ds_sel);
        ES::set_reg(kernel_ds_sel);
        SS::set_reg(kernel_ds_sel);
        // FS::set_reg(kernel_ds_sel);
        // GS::set_reg(kernel_ds_sel);

        load_tss(tss_sel);
    }
}
