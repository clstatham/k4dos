use crate::{util::{ctypes::c_int, KResult, errno::Errno}, fs::opened_file::{FileDesc, OpenFlags, OpenOptions}, task::current_task, errno};

use super::SyscallHandler;


const F_DUPFD: c_int = 0;
const F_GETFD: c_int = 1;
const F_SETFD: c_int = 2;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;

// Linux-specific commands.
const F_LINUX_SPECIFIC_BASE: c_int = 1024;
const F_DUPFD_CLOEXEC: c_int = F_LINUX_SPECIFIC_BASE + 6;

impl<'a> SyscallHandler<'a> {
    pub fn sys_fcntl(&mut self, fd: FileDesc, cmd: c_int, arg: usize) -> KResult<isize> {
        let current = current_task();
        let mut files = current.opened_files.lock();
        match cmd {
            F_GETFD => {
                let flags = files.get(fd)?.get_flags();
                Ok(flags.bits() as isize)
            }
            F_SETFD => {
                files.get(fd)?.set_close_on_exec(arg == 1);
                Ok(0)
            }
            F_SETFL => {
                files
                    .get(fd)?
                    .set_flags(OpenFlags::from_bits_truncate(arg as i32))?;
                Ok(0)
            }
            F_DUPFD_CLOEXEC => {
                let fd = files.dup(fd, Some(arg as i32), OpenOptions::new(false, true))?;
                Ok(fd as isize)
            }
            _ => Err(errno!(Errno::ENOSYS)),
        }
    }
}
