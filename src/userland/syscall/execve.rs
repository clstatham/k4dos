use core::{mem::size_of, ops::Add};

use alloc::vec::Vec;

use crate::{
    fs::{initramfs::get_root, path::Path},
    mem::addr::VirtAddr,
    task::current_task,
    userland::buffer::UserCStr,
    util::KResult,
};

use super::SyscallHandler;

const ARG_MAX: usize = 512;
const ARG_LEN_MAX: usize = 4096;
const ENV_MAX: usize = 512;
const ENV_LEN_MAX: usize = 4096;

impl<'a> SyscallHandler<'a> {
    pub fn sys_execve(
        &mut self,
        path: &Path,
        argv_addr: VirtAddr,
        envp_addr: VirtAddr,
    ) -> KResult<isize> {
        let current = current_task();
        log::debug!("Statting path {}", path);
        let exefile = current_task().root_fs.lock().lookup(path)?.as_file()?.clone();

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
}
