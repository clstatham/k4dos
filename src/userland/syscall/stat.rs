use crate::{fs::{path::Path, initramfs::get_root, opened_file::FileDesc}, mem::addr::VirtAddr, util::KResult, task::current_task};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_stat(&mut self, path: &Path, buf: VirtAddr) -> KResult<isize> {
        let stat = current_task().root_fs.lock().lookup(path)?.stat()?;
        buf.write(stat)?;
        Ok(0)
    }

    pub fn sys_fstat(&mut self, fd: FileDesc, buf: VirtAddr) -> KResult<isize> {
        let file = current_task().get_opened_file_by_fd(fd)?;
        let stat = file.path().inode.stat()?;
        buf.write(stat)?;
        Ok(0)
    }
}