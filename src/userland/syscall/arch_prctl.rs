use alloc::sync::Arc;

use crate::{
    errno,
    mem::addr::VirtAddr,
    task::{current_task, Task},
    util::{errno::Errno, KResult},
};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_arch_prctl(&mut self, code: i32, uaddr: VirtAddr) -> KResult<isize> {
        arch_prctl(&current_task(), code, uaddr)?;
        Ok(0)
    }
}

fn arch_prctl(current_task: &Arc<Task>, code: i32, addr: VirtAddr) -> KResult<()> {
    const ARCH_SET_FS: i32 = 0x1002;

    match code {
        ARCH_SET_FS => {
            current_task.arch_mut().set_fsbase(addr);
            // unsafe {
            //     wrfsbase(value as u64);
            // }
            // let vmem = current.vmem();
            // let mut vmem = vmem.as_ref().unwrap().lock();
            // let fsbase_page = UserPage::containing_address(uaddr);
            // let range = UserPageRange::new(fsbase_page, fsbase_page + 1);
            // if vmem.is_range_free(&range) {
            //     vmem.allocate_new_area(range, VirtualMemoryType::Other, PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE, None)?;
            // }
        }
        _ => return Err(errno!(Errno::EINVAL)),
    }

    Ok(())
}
