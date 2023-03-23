use alloc::{sync::Arc, borrow::ToOwned};

use crate::{fs::{path::Path, opened_file::{OpenFlags, FileDesc}, FileMode, INode, initramfs::{file::InitRamFsFile}, alloc_inode_no, O_WRONLY, O_RDWR}, util::{KResult, errno::Errno}, errno, task::current_task};

use super::SyscallHandler;


fn create(path: &Path, flags: OpenFlags, _mode: FileMode) -> KResult<INode> {
    if flags.contains(OpenFlags::O_DIRECTORY) {
        return Err(errno!(Errno::EINVAL, "create(): invalid flags"));
    }

    let (_parent_dir, name) = path
        .parent_and_basename()
        .ok_or_else(|| errno!(Errno::EEXIST, "create(): invalid path"))?;

    let current = current_task();
    let root = current.root_fs.lock();
    let inode = INode::File(Arc::new(InitRamFsFile::new(name.to_owned(), alloc_inode_no())));
    root
        .lookup(path, true)?
        .as_dir()?
        .insert(inode.clone());
    Ok(inode)
}

impl<'a> SyscallHandler<'a> {
    pub fn sys_open(&mut self, path: &Path, flags: OpenFlags, mode: FileMode) -> KResult<isize> {
        let current = current_task();
        log::trace!("Attempting to open {}", path);
        if flags.contains(OpenFlags::O_CREAT) {
            match create(path, flags, mode) {
                Ok(_) => {},
                Err(err) if flags.contains(OpenFlags::O_EXCL) && err.errno() == Some(Errno::EEXIST) => {},
                Err(err) => return Err(err),
            }
        }

        let root = current.root_fs.lock();
        let mut opened_files = current.opened_files.lock();
        let path_comp = root.lookup_path(path, true)?;
        if flags.contains(OpenFlags::O_DIRECTORY) && !path_comp.inode.is_dir() {
            return Err(errno!(Errno::ENOTDIR, "sys_open(): not a directory"));
        }
        let access_mode = mode.access_mode();
        if path_comp.inode.is_dir() && (access_mode == O_WRONLY || access_mode == O_RDWR) {
            return Err(errno!(Errno::EISDIR, "sys_open(): is a directory"));
        }

        let fd = opened_files.open(path_comp, flags.into())?;
        log::trace!("Opened {} as {}.", path, fd);
        Ok(fd as isize)
    }

    pub fn sys_close(&mut self, fd: FileDesc) -> KResult<isize> {
        let current = current_task();
        current.opened_files.lock().close(fd)?;
        log::trace!("Closed {}", fd);
        Ok(0)
    }
}