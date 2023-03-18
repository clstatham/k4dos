use x86::msr::{rdmsr, IA32_GS_BASE};
use x86_64::structures::{
    gdt::GlobalDescriptorTable, idt::InterruptDescriptorTable, tss::TaskStateSegment,
};

use crate::task::scheduler::Scheduler;

#[repr(C)]
pub struct Kpcr {
    pub user_rsp_tmp: usize,
    pub kernel_sp: usize,
    pub tss: TaskStateSegment,
    pub gdt: GlobalDescriptorTable,
    pub idt: InterruptDescriptorTable,
    pub scheduler: Scheduler,
}

pub fn kpcr() -> &'static mut Kpcr {
    unsafe { &mut *(rdmsr(IA32_GS_BASE) as *mut _) }
}
