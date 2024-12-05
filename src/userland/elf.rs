use alloc::{borrow::ToOwned, string::String, vec::Vec};
use elfloader::{ElfBinary, ElfLoader, Entry};
use x86::random::rdrand_slice;

use xmas_elf::{
    program::Type,
    sections::{SectionData, ShType},
};

use crate::{
    fs::{initramfs::get_root, opened_file::OpenFlags, path::Path, FileRef},
    kerror,
    mem::{
        addr::VirtAddr, addr_space::AddressSpace, allocator::alloc_kernel_frames, consts::PAGE_SIZE,
    },
    task::vmem::{MMapFlags, MMapKind, MMapProt, Vmem},
    userland::buffer::UserBufferMut,
    util::{align_up, KResult},
};

pub fn gen_stack_canary() -> [u8; 16] {
    let mut random_bytes = [0u8; 16];
    unsafe { rdrand_slice(&mut random_bytes) };
    random_bytes
}

#[repr(u64)]
#[derive(Debug, Copy, Clone)]
pub enum AuxvType {
    AtNull = 0,
    AtPhdr = 3,
    AtPhEnt = 4,
    AtPhNum = 5,
    AtEntry = 9,
}

#[derive(Clone)]
pub struct SymTabEntry {
    pub name: String,
    pub value: u64,
    pub size: u64,
}

pub struct UserlandEntry {
    pub entry_point: VirtAddr,
    pub vmem: Vmem,
    pub fsbase: Option<VirtAddr>,
    pub addr_space: AddressSpace,
    pub hdr: [(AuxvType, usize); 4],
    pub symtab: Option<Vec<SymTabEntry>>,
}

pub fn load_elf(file: FileRef) -> KResult<UserlandEntry> {
    let len = file.stat()?.size.0 as usize;
    let current = AddressSpace::current();
    // let mut buf = alloc::vec![0u8; len];
    let mut addr_space = AddressSpace::new()?;
    addr_space.switch();
    let frames = alloc_kernel_frames(align_up(len, PAGE_SIZE) / PAGE_SIZE)?;

    // let mp = addr_space.mapper().map(pages, PageTableFlags::PRESENT | PageTableFlags::WRITABLE)?;
    let buf = unsafe {
        core::slice::from_raw_parts_mut(
            frames.start_address().as_hhdm_virt().as_raw_ptr_mut(),
            frames.size_in_bytes(),
        )
    };
    let ubuf = UserBufferMut::from_slice(buf);
    file.read(0, ubuf, &OpenFlags::empty())?;
    current.switch();
    let elf = ElfBinary::new(buf).map_err(|_e| kerror!("load_elf(): elf loader error"))?;

    let mut start_of_image = usize::MAX;
    let mut end_of_image = 0;
    for hdr in elf.program_headers() {
        if hdr.get_type().unwrap() == xmas_elf::program::Type::Load {
            end_of_image = end_of_image.max((hdr.virtual_addr() + hdr.mem_size()) as usize);
            start_of_image = end_of_image.min(hdr.virtual_addr() as usize);
        }
    }
    let mut symbol_table = None;
    for section in elf.file.section_iter() {
        if section.get_type() == Ok(ShType::SymTab) {
            let section_data = section.get_data(&elf.file);
            if let Ok(ref _section_data @ SectionData::SymbolTable64(symtab)) = section_data {
                symbol_table = Some(
                    symtab
                        .iter()
                        .map(|e| SymTabEntry {
                            name: e.get_name(&elf.file).unwrap().to_owned(),
                            size: e.size(),
                            value: e.value(),
                        })
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
    if symbol_table.is_none() {
        log::warn!("Couldn't get symbol table for ELF.");
    }
    log::debug!(
        "ELF loaded into memory at {:#x} .. {:#x}",
        start_of_image,
        end_of_image
    );
    if elf.is_pie() {
        log::warn!("It's a PIE");
    }

    let mut vmem = Vmem::new();

    let load_offset =
        if elf.file.header.pt2.type_().as_type() == xmas_elf::header::Type::SharedObject {
            0x40000000
        } else {
            0
        };

    let entry_point = VirtAddr::new(elf.entry_point() as usize + load_offset);

    log::debug!("Entry point: {:?}", entry_point);

    addr_space.switch();
    let mut loader = KadosElfLoader {
        vmem: &mut vmem,
        addr_space: &mut addr_space,
        base_addr: usize::MAX,
        load_offset,
        file: file.clone(),
        entry_point,
    };

    elf.load(&mut loader).unwrap();

    let p2 = elf.file.header.pt2;
    log::debug!("Base address at {:?}", VirtAddr::new(loader.base_addr));
    let hdr = [
        (AuxvType::AtPhdr, p2.ph_offset() as usize + loader.base_addr),
        (AuxvType::AtPhEnt, p2.ph_entry_size() as usize),
        (AuxvType::AtPhNum, p2.ph_count() as usize),
        (AuxvType::AtEntry, p2.entry_point() as usize),
    ];

    if let Some(ref symtab) = symbol_table {
        for sym in symtab.iter() {
            if sym.name == "__stack_chk_fail" {
                // unsafe {
                //     *(sym.value as *mut u8) = 0xc3; // "ret" instruction
                // }
                log::warn!("SSP is ON for this binary!");
                break;
            }
        }
    }
    current.switch();

    log::debug!("ELF load complete.");
    Ok(UserlandEntry {
        entry_point: loader.entry_point,
        vmem,
        fsbase: None,
        addr_space,
        hdr,
        symtab: symbol_table,
    })
}

struct KadosElfLoader<'a> {
    vmem: &'a mut Vmem,
    addr_space: &'a mut AddressSpace,
    base_addr: usize,
    load_offset: usize,
    file: FileRef,
    entry_point: VirtAddr,
}

impl ElfLoader for KadosElfLoader<'_> {
    fn allocate(
        &mut self,
        load_headers: elfloader::LoadableHeaders,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        for header in load_headers {
            if header.get_type().unwrap() == Type::Load {
                let start = VirtAddr::new(header.virtual_addr() as usize + self.load_offset)
                    .align_down(PAGE_SIZE);
                let mem_end = VirtAddr::new(
                    header.virtual_addr() as usize + header.mem_size() as usize + self.load_offset,
                )
                .align_up(PAGE_SIZE);
                if start.value() < self.base_addr {
                    self.base_addr = start.value();
                }
                let flags = MMapFlags::empty();
                let mut prot = MMapProt::PROT_WRITE;
                if header.flags().is_execute() {
                    prot.insert(MMapProt::PROT_EXEC);
                }
                let kind = MMapKind::File {
                    file: self.file.clone(),
                    offset: header.offset() as usize,
                    size: header.file_size() as usize,
                };
                log::debug!("Mapping region {:?} .. {:?}", start, mem_end);
                self.addr_space.with_mapper(|mut mapper| {
                    self.vmem
                        .map_area(start, mem_end, flags, prot, kind, &mut mapper)
                        .unwrap();
                });
            } else if header.get_type().unwrap() == Type::Interp {
                let ld = get_root()
                    .unwrap()
                    .lookup(Path::new("/usr/lib/ld.so"), true)
                    .unwrap()
                    .as_file()
                    .unwrap()
                    .clone();
                let res = load_elf(ld).unwrap();
                self.entry_point = res.entry_point;
            }
        }

        Ok(())
    }

    fn load(
        &mut self,
        flags: elfloader::Flags,
        base: elfloader::VAddr,
        region: &[u8],
    ) -> Result<(), elfloader::ElfLoaderErr> {
        let region_start = VirtAddr::new(base as usize + self.load_offset);
        let region_end = region_start + region.len();
        let area = self
            .vmem
            .area_containing_mut(region_start, region_end)
            .unwrap();
        let mut prot = MMapProt::empty();
        if flags.is_read() {
            prot |= MMapProt::PROT_READ;
        }
        if flags.is_write() {
            prot |= MMapProt::PROT_WRITE;
        }
        if flags.is_execute() {
            prot |= MMapProt::PROT_EXEC;
        }
        // this should be safe since the pages should already be mapped and writable in allocate()
        unsafe { region_start.write_bytes(region).unwrap() };
        // set the correct protections now
        area.prot = prot;
        Ok(())
    }

    fn tls(
        &mut self,
        tdata_start: elfloader::VAddr,
        _tdata_length: u64,
        _total_size: u64,
        _align: u64,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        log::warn!(
            "TLS section found at {:?}",
            VirtAddr::new(tdata_start as usize)
        );
        Ok(())
    }

    fn relocate(
        &mut self,
        entry: elfloader::RelocationEntry,
    ) -> Result<(), elfloader::ElfLoaderErr> {
        use elfloader::arch::x86_64::RelocationTypes;
        match entry.rtype {
            elfloader::RelocationType::x86_64(rtype) => match rtype {
                RelocationTypes::R_AMD64_RELATIVE => {
                    let reloc_value = entry.addend.unwrap() as usize + self.load_offset;
                    log::trace!(
                        "Applying relocation R_AMD64_RELATIVE at location {:#x} -> {:#x}",
                        entry.offset,
                        reloc_value
                    );
                    unsafe {
                        *((entry.offset + self.load_offset as u64) as *mut usize) = reloc_value;
                    }
                }
                rtype => {
                    log::error!("Unsupported relocation type: {:?}", rtype);
                    return Err(elfloader::ElfLoaderErr::UnsupportedRelocationEntry);
                }
            },
            _ => return Err(elfloader::ElfLoaderErr::UnsupportedArchitecture),
        }
        Ok(())
    }
}
