use crate::{
    fs::opened_file::FileDesc,
    mem::addr::VirtAddr,
    task::{
        current_task,
        vmem::{MMapFlags, MMapProt},
    },
    userland::syscall::SyscallHandler,
    util::KResult,
};

impl SyscallHandler<'_> {
    pub fn sys_mmap(
        &mut self,
        addr: VirtAddr,
        size: usize,
        prot: MMapProt,
        flags: MMapFlags,
        fd: FileDesc,
        offset: usize,
    ) -> KResult<isize> {
        if fd as isize != -1 {
            todo!("mmap file");
        }

        current_task()
            .vmem()
            .lock()
            .mmap(addr, size, prot, flags, fd, offset)
            .map(|addr| addr.value() as isize)
    }

    pub fn sys_mprotect(&mut self, addr: VirtAddr, size: usize, prot: MMapProt) -> KResult<isize> {
        current_task().vmem().lock().mprotect(addr, size, prot)?;
        Ok(0)
    }

    pub fn sys_munmap(&mut self, addr: VirtAddr, size: usize) -> KResult<isize> {
        let current = current_task();
        current.vmem().lock().munmap(
            &mut current.arch_mut().address_space.mapper(),
            addr,
            addr + size,
        )?;
        Ok(0)
    }

    pub fn sys_mremap(&mut self, addr: VirtAddr, old_size: usize, size: usize) -> KResult<isize> {
        let current = current_task();
        let new_addr = current.vmem().lock().mremap(
            addr,
            old_size,
            size,
            &mut current.arch_mut().address_space.mapper(),
        )?;
        Ok(new_addr.value() as isize)
    }
}
