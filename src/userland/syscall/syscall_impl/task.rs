use core::{mem::size_of, ops::Add};

use alloc::{sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::{
    errno,
    fs::path::Path,
    mem::addr::VirtAddr,
    task::{current_task, get_scheduler, group::PgId, Task, TaskId, TaskState, JOIN_WAIT_QUEUE},
    userland::{buffer::UserCStr, syscall::SyscallHandler},
    util::{ctypes::c_int, errno::Errno, KResult},
};

const ARG_MAX: usize = 512;
const ARG_LEN_MAX: usize = 4096;
const ENV_MAX: usize = 512;
const ENV_LEN_MAX: usize = 4096;

impl<'a> SyscallHandler<'a> {
    pub fn sys_arch_prctl(&mut self, code: i32, uaddr: VirtAddr) -> KResult<isize> {
        arch_prctl(current_task(), code, uaddr)?;
        Ok(0)
    }

    pub fn sys_fork(&mut self) -> KResult<isize> {
        let child = current_task().fork();
        Ok(child.pid().as_usize() as isize)
    }

    pub fn sys_clone(
        &mut self,
        _clone_flags: usize,
        user_stack: VirtAddr,
        r8: usize,
        args: VirtAddr,
        r9: usize,
        entry_point: VirtAddr,
    ) -> KResult<isize> {
        let child = current_task().clone_process(entry_point, user_stack, args, r8, r9, self.frame);
        Ok(child.pid().as_usize() as isize)
    }

    pub fn sys_execve(
        &mut self,
        path: &Path,
        argv_addr: VirtAddr,
        envp_addr: VirtAddr,
    ) -> KResult<isize> {
        let current = current_task();
        log::debug!("Statting path {}", path);
        let exefile = current_task()
            .root_fs
            .lock()
            .lookup(path, true)?
            .as_file()?
            .clone();

        let mut argv = Vec::new();
        for i in 0..ARG_MAX {
            let ptr = argv_addr.add(i * size_of::<usize>());
            let str_ptr = ptr.read::<usize>()?;
            if *str_ptr != 0 {
                argv.push(UserCStr::new(VirtAddr::new(*str_ptr), ARG_LEN_MAX)?);
            } else {
                break;
            }
        }

        let mut envp = Vec::new();
        for i in 0..ENV_MAX {
            let ptr = envp_addr.add(i * size_of::<usize>());
            let str_ptr = ptr.read::<usize>()?;
            if *str_ptr != 0 {
                envp.push(UserCStr::new(VirtAddr::new(*str_ptr), ENV_LEN_MAX)?);
            } else {
                break;
            }
        }
        let argv: Vec<&[u8]> = argv.as_slice().iter().map(|s| s.as_bytes()).collect();
        let envp: Vec<&[u8]> = envp.as_slice().iter().map(|s| s.as_bytes()).collect();
        current.exec(exefile, &argv, &envp)?;
        Ok(0)
    }

    pub fn sys_exit(&mut self, status: c_int) -> KResult<isize> {
        get_scheduler().exit_current(status);
        Ok(0)
    }

    pub fn sys_set_tid_address(&mut self, _addr: VirtAddr) -> KResult<isize> {
        Ok(current_task().pid().as_usize() as isize)
    }

    pub fn sys_getpid(&mut self) -> KResult<isize> {
        Ok(current_task().pid().as_usize() as isize)
    }

    pub fn sys_getppid(&mut self) -> KResult<isize> {
        Ok(current_task().ppid().as_usize() as isize)
    }

    pub fn sys_getpgid(&mut self, pid: TaskId) -> KResult<isize> {
        if pid.as_usize() == 0 {
            Ok(current_task().pgid().unwrap() as isize)
        } else {
            Ok(get_scheduler()
                .find_task(pid)
                .ok_or(errno!(Errno::ESRCH))?
                .pgid()
                .unwrap() as isize)
        }
    }

    pub fn sys_setpgid(&mut self, pid: TaskId, pgid: PgId) -> KResult<isize> {
        if pid.as_usize() == 0 {
            current_task()
                .group
                .borrow_mut()
                .upgrade()
                .unwrap()
                .lock()
                .set_pgid(pgid);
        } else {
            get_scheduler()
                .find_task(pid)
                .ok_or(errno!(Errno::ESRCH))?
                .group
                .borrow_mut()
                .upgrade()
                .unwrap()
                .lock()
                .set_pgid(pgid);
        }
        Ok(0)
    }
}

bitflags! {
    pub struct WaitOptions: c_int {
        const WNOHANG   = 1;
        const WUNTRACED = 2;
    }
}

impl<'a> SyscallHandler<'a> {
    pub fn sys_wait4(
        &mut self,
        pid: TaskId,
        status: VirtAddr,
        options: WaitOptions,
        _rusage: VirtAddr, // could be null
    ) -> KResult<isize> {
        let (got_pid, status_val) = JOIN_WAIT_QUEUE.sleep_signalable_until(None, || {
            let current = current_task();
            let children = current.children.lock();
            if children.is_empty() {
                return Err(errno!(Errno::ECHILD))
            }
            for child in children.iter() {
                if pid.as_usize() as isize > 0 && pid != child.pid() {
                    continue;
                }

                if pid.as_usize() == 0 {
                    todo!()
                }

                if let TaskState::ExitedWith(status_val) = child.get_state() {
                    return Ok(Some((child.pid(), status_val)));
                }
            }

            if options.contains(WaitOptions::WNOHANG) {
                return Ok(Some((TaskId::new(0), 0)));
            }

            Ok(None)
        })?;

        log::debug!("wait4: status = {status_val}");
        current_task()
            .children
            .lock()
            .retain(|p| p.pid() != got_pid);

        if status.value() != 0 {
            status.write::<c_int>(status_val)?;
        }

        Ok(got_pid.as_usize() as isize)
    }
}

fn arch_prctl(current_task: Arc<Task>, code: i32, addr: VirtAddr) -> KResult<()> {
    const ARCH_SET_FS: i32 = 0x1002;

    match code {
        ARCH_SET_FS => {
            current_task.arch_mut().set_fsbase(addr);
        }
        _ => return Err(errno!(Errno::EINVAL, "arch_prctl(): unknown code")),
    }

    Ok(())
}
