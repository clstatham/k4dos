
use crate::{fs::opened_file::FileDesc, util::KResult, task::current_task};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_ioctl(&mut self, fd: FileDesc, cmd: usize, arg: usize) -> KResult<isize> {
        let opened_file = current_task().get_opened_file_by_fd(fd)?;
        opened_file.ioctl(cmd, arg)
    }
}
