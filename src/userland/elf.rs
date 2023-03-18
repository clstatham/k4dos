use core::ops::Add;

use elfloader::{ElfBinary, ElfLoader};
use x86::random::rdrand_slice;
use x86_64::structures::paging::PageTableFlags;
use xmas_elf::{program::Type, header::HeaderPt2};

use crate::{mem::{addr::VirtAddr, allocator::AllocationError, addr_space::AddressSpace, paging::table::PagingError, consts::PAGE_SIZE}, task::vmem::Vmem, fs::{FileRef, opened_file::OpenOptions}, util::{KResult, KError, errno::Errno, align_up, align_down}, kerr, errno, userland::buffer::UserBufferMut};

#[derive(Debug)]
pub enum ElfLoadError {
    FrameAllocationFailed(KError<AllocationError>),
    PagingError(KError<PagingError>),
}

impl Into<KError<ElfLoadError>> for KError<AllocationError> {
    fn into(self) -> KError<ElfLoadError> {
        kerr!(ElfLoadError::FrameAllocationFailed(self))
    }
}

impl Into<KError<ElfLoadError>> for KError<()> {
    fn into(self) -> KError<ElfLoadError> {
        errno!(self.errno().unwrap())
    }
}

impl Into<KError<ElfLoadError>> for KError<PagingError> {
    fn into(self) -> KError<ElfLoadError> {
        kerr!(ElfLoadError::PagingError(self))
    }
}

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

pub struct UserlandEntry {
    pub entry_point: VirtAddr,
    pub vmem: Vmem,
    pub fsbase: Option<VirtAddr>,
    pub addr_space: AddressSpace,
    pub hdr: [(AuxvType, usize); 4],
}

pub fn load_elf<'a>(file: FileRef, argv: &[&[u8]], envp: &[&[u8]]) -> KResult<UserlandEntry, ElfLoadError> {
    let len = file.stat().map_err(|e| e.into())?.size.0 as usize;
    let mut buf = alloc::vec![0u8; len];
    let ubuf = UserBufferMut::from_slice(&mut buf);
    file.read(0, ubuf, &OpenOptions::empty()).map_err(|e| e.into())?;

    let elf = ElfBinary::new(&buf).map_err(|e| errno!(Errno::EBADF))?;

    let mut start_of_image = usize::MAX;
    let mut end_of_image = 0;
    for hdr in elf.program_headers() {
        if hdr.get_type().unwrap() == xmas_elf::program::Type::Load {
            end_of_image = end_of_image.max((hdr.virtual_addr() + hdr.mem_size()) as usize);
            start_of_image = end_of_image.min(hdr.virtual_addr() as usize);
        }
    }
    log::debug!(
        "ELF loaded into memory at {:#x} .. {:#x}",
        start_of_image, end_of_image
    );
    if elf.is_pie() {
        log::warn!("It's a PIE");
    }

    let mut vmem = Vmem::new();
    let mut addr_space = AddressSpace::new().map_err(|e| e.into())?;

    // let user_heap_bottom = align_up(end_of_image, PAGE_SIZE);
    // let random_bytes = gen_stack_canary();

    let entry_point = VirtAddr::new(elf.entry_point() as usize);

    log::debug!("Entry point: {:?}", entry_point);
    let current = AddressSpace::current();
    addr_space.switch();
    let mut loader = KadosElfLoader {
        vmem: &mut vmem,
        addr_space: &mut addr_space,
    };
    
    elf.load(&mut loader).unwrap();

    current.switch();
    let p2 = elf.file.header.pt2.clone();
    let hdr = [
        (AuxvType::AtPhdr, p2.ph_offset() as usize + start_of_image),
        (AuxvType::AtPhEnt, p2.ph_entry_size() as usize),
        (AuxvType::AtPhNum, p2.ph_count() as usize),
        (AuxvType::AtEntry, p2.entry_point() as usize),
    ];

    log::debug!("ELF load complete.");
    Ok(UserlandEntry { entry_point, vmem, fsbase: None, addr_space, hdr })
}

struct KadosElfLoader<'a> {
    vmem: &'a mut Vmem,
    addr_space: &'a mut AddressSpace,
}

impl<'a> ElfLoader for KadosElfLoader<'a> {
    fn allocate(&mut self, load_headers: elfloader::LoadableHeaders) -> Result<(), elfloader::ElfLoaderErr> {
        for header in load_headers.filter(|header| header.get_type().unwrap() == Type::Load) {
            let start = VirtAddr::new(header.virtual_addr() as usize).align_down(PAGE_SIZE);
            let mem_end = VirtAddr::new(header.virtual_addr() as usize + header.mem_size() as usize).align_up(PAGE_SIZE);
            // let file_end = VirtAddr::new(header.virtual_addr() as usize + header.file_size() as usize).align_up(PAGE_SIZE);
            // let data_size = file_end - start;
            // let aligned_data_size = align_up(data_size, PAGE_SIZE);
            // let file_offset = align_down(header.offset() as usize, PAGE_SIZE);
            let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE;
            // if header.flags().is_write() {
            //     flags.insert(PageTableFlags::WRITABLE);
            // }
            if !header.flags().is_execute() {
                flags.insert(PageTableFlags::NO_EXECUTE);
            }
            log::debug!("Mapping region {:?} .. {:?}", start, mem_end);
            self.vmem.map_area(start, mem_end, flags, &mut self.addr_space.mapper()).unwrap();
        }

        Ok(())
    }

    fn load(&mut self, flags: elfloader::Flags, base: elfloader::VAddr, region: &[u8]) -> Result<(), elfloader::ElfLoaderErr> {
        let region_start = VirtAddr::new(base as usize);
        // let region_end = region_start + region.len();
        // let area_id = self.vmem.area_containing(region_start, region_end).unwrap();
        // let area = self.vmem.area(area_id).unwrap();
        // let area_start = area.start_address();
        // let offset = region_start - area_start;
        // this should be safe since the pages should already be mapped in allocate()
        region_start.as_bytes_mut(region.len()).unwrap().copy_from_slice(region);
        Ok(())
    }

    // fn make_readonly(&mut self, base: elfloader::VAddr, size: usize) -> Result<(), elfloader::ElfLoaderErr> {
    //     let start = VirtAddr::new(base as usize);
    //     let end = start + size;
    //     let area_id = self.vmem.area_containing(start, end).unwrap();
    //     // todo
    // }

    fn tls(
            &mut self,
            _tdata_start: elfloader::VAddr,
            _tdata_length: u64,
            _total_size: u64,
            _align: u64,
        ) -> Result<(), elfloader::ElfLoaderErr> {
        Ok(())
    }

    fn relocate(&mut self, entry: elfloader::RelocationEntry) -> Result<(), elfloader::ElfLoaderErr> {
        Ok(())
    }
}