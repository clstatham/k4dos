use core::{alloc::Layout, mem::size_of, panic::PanicInfo};

use alloc::vec::Vec;
use spin::Once;
use x86_64::instructions::interrupts;
use xmas_elf::{
    sections::{SectionData, ShType},
    symbol_table::Entry,
    ElfFile,
};

use crate::{
    kerrmsg,
    mem::{addr::VirtAddr, addr_space::AddressSpace, consts::PAGE_SIZE},
    task::get_scheduler,
    userland::elf::SymTabEntry,
    util::{KResult, SavedInterruptStatus},
};

pub static KERNEL_ELF: Once<ElfFile<'static>> = Once::new();

fn print_symbol(rip: usize, symtab: &Option<Vec<SymTabEntry>>, depth: usize) {
    if let Some(ref symbol_table) = symtab {
        let mut name = None;
        for data in symbol_table {
            let st_value = data.value as usize;
            let st_size = data.size as usize;

            if rip >= st_value && rip < (st_value + st_size) {
                name = Some(data.name.clone());
            }
        }

        if let Some(name) = name {
            serial0_println!("{:>2}: 0x{:016x} - {}", depth, rip, name);
        } else {
            serial0_println!(
                "{:>2}: 0x{:016x} - <unknown> (symbol not found)",
                depth,
                rip
            );
        }
    } else {
        serial0_println!("{:>2}: 0x{:016x} - <unknown> (no symbol table)", depth, rip);
    }
}

pub fn unwind_user_stack_from(mut rbp: usize, mut rip: usize) {
    let _guard = SavedInterruptStatus::save();
    interrupts::disable();
    let mut addr_space = AddressSpace::current();
    let pt = addr_space.mapper();

    if rbp == 0 {
        serial0_println!("<empty backtrace>");
        return;
    }

    let current = get_scheduler().current_task_opt();
    let symtab = if let Some(current) = current {
        let s = current.arch_mut().symtab.clone();
        if s.is_none() {
            serial0_println!(
                "Warning: Couldn't find symbol table for pid {}",
                current.pid().as_usize()
            );
        }
        s
    } else {
        serial0_println!("Warning: Couldn't lock current scheduler task");
        None
    };

    serial0_println!("---BEGIN BACKTRACE---");
    print_symbol(rip, &symtab, 0);
    for depth in 1..17 {
        if let Some(rip_rbp) = rbp.checked_add(size_of::<usize>()) {
            if rip_rbp < PAGE_SIZE || pt.translate(VirtAddr::new(rip_rbp)).is_none() {
                serial0_println!("{:>2}: <guard page>", depth);
                break;
            }

            rip = unsafe { *(rip_rbp as *const usize) };
            if rip == 0 || rbp == 0 {
                break;
            }

            unsafe {
                rbp = *(rbp as *const usize);
            }

            print_symbol(rip, &symtab, depth);
        } else {
            break;
        }
    }
    serial0_println!("---END BACKTRACE---");
}

pub fn unwind_stack() -> KResult<()> {
    let _guard = SavedInterruptStatus::save();
    interrupts::disable();
    let mut addr_space = AddressSpace::current();
    let pt = addr_space.mapper();

    let kernel_elf = KERNEL_ELF
        .get()
        .ok_or(kerrmsg!("KERNEL_ELF not initialized"))?;
    let mut symbol_table = None;

    for section in kernel_elf.section_iter() {
        if section.get_type() == Ok(ShType::SymTab) {
            let section_data = section
                .get_data(kernel_elf)
                .map_err(|_| kerrmsg!("Failed to get kernel section data"))?;

            if let SectionData::SymbolTable64(symtab) = section_data {
                symbol_table = Some(symtab);
            }
        }
    }

    let symbol_table = symbol_table.ok_or(kerrmsg!("No symbol table available"))?;
    let mut rbp: usize;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
    }

    if rbp == 0 {
        serial0_println!("<empty backtrace>");
        return Ok(());
    }

    serial0_println!("---BEGIN BACKTRACE---");
    for depth in 0..16 {
        if let Some(rip_rbp) = rbp.checked_add(size_of::<usize>()) {
            if pt.translate(VirtAddr::new(rip_rbp)).is_none() {
                serial0_println!("{:>2}: <guard page>", depth);
                break;
            }

            let rip = unsafe { *(rip_rbp as *const usize) };
            if rip == 0 || rbp == 0 {
                break;
            }

            unsafe {
                rbp = *(rbp as *const usize);
            }

            let mut name = None;
            for data in symbol_table {
                let st_value = data.value() as usize;
                let st_size = data.size() as usize;

                if rip >= st_value && rip < (st_value + st_size) {
                    let mangled_name = data.get_name(kernel_elf).unwrap_or("<unknown>");
                    name = Some(rustc_demangle::demangle(mangled_name));
                }
            }

            if let Some(name) = name {
                serial0_println!("{:>2}: 0x{:016x} - {}", depth, rip, name);
            } else {
                serial0_println!("{:>2}: 0x{:016x} - <unknown>", depth, rip);
            }
        } else {
            break;
        }
    }
    serial0_println!("---END BACKTRACE---");

    Ok(())
}

#[panic_handler]
fn rust_panic(info: &PanicInfo) -> ! {
    interrupts::disable();
    let panic_msg = info.message();

    serial0_println!("Panicked at '{}'", panic_msg);
    // if FRAMEBUFFER.get().is_some() {
    //     fb_println!("Panicked at '{}'", panic_msg);
    // }

    if let Some(panic_location) = info.location() {
        serial0_println!("{}", panic_location);
        // if FRAMEBUFFER.get().is_some() {
        //     fb_println!("{}", panic_location);
        // }
    }

    // serial0_println!("");
    match unwind_stack() {
        Ok(()) => {}
        Err(e) => serial0_println!("Error unwinding stack: {:?}", e.msg()),
    }

    crate::hcf();
}

#[allow(non_snake_case)]
#[no_mangle]
extern "C" fn _Unwind_Resume(unwind_context_ptr: usize) -> ! {
    serial0_println!("{:#x}", unwind_context_ptr);
    crate::hcf();
}

#[lang = "eh_personality"]
#[no_mangle]
extern "C" fn rust_eh_personality() -> ! {
    serial0_println!("Poisoned function `rust_eh_personality` was called.");
    crate::hcf()
}

#[alloc_error_handler]
fn handle_alloc_error(layout: Layout) -> ! {
    panic!("Alloc Error for layout {:?}", layout)
}
