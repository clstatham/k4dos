use x86::msr::{rdmsr, IA32_GS_BASE};
use x86_64::structures::{
    gdt::GlobalDescriptorTable, tss::TaskStateSegment,
};

pub struct CpuLocalData {
    pub kernel_sp: usize,
    pub gdt: GlobalDescriptorTable,
}

#[repr(C, packed)]
pub struct Kpcr {
    pub tss: TaskStateSegment,
    pub cpu_local: &'static mut CpuLocalData,
}

pub fn get_kpcr() -> &'static mut Kpcr {
    unsafe { &mut *(rdmsr(IA32_GS_BASE) as *mut _) }
}

pub fn get_tss() -> &'static mut TaskStateSegment {
    unsafe { &mut *(rdmsr(IA32_GS_BASE) as *mut _) }
}
