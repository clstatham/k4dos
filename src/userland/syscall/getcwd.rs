use core::mem::size_of;

use alloc::string::String;

use crate::{mem::addr::VirtAddr, util::{KResult, errno::Errno, align_up}, fs::{initramfs::get_root, opened_file::{OpenOptions, FileDesc}, path::Path}, errno, userland::buffer::{UserBufferMut, UserBufferWriter}, task::current_task};

use super::SyscallHandler;


impl<'a> SyscallHandler<'a> {
    pub fn sys_getcwd(&mut self, buf: VirtAddr, len: u64) -> KResult<isize> {
        let cwd = current_task().root_fs.lock()
            .cwd_path()
            .resolve_abs_path();

        if (len as usize) < cwd.as_str().as_bytes().len() {
            return Err(errno!(Errno::ERANGE));
        }

        let mut cwd = String::from(cwd.as_str());
        cwd.push('\0');
        let buf_val = buf.value();
        let mut writer = UserBufferMut::from_vaddr(buf, len as usize);
        writer
            .write_at(cwd.as_str().as_bytes(), 0, &OpenOptions::empty())
            .unwrap(); // this currently never returns Err; may change
        Ok(buf_val as isize)
    }

    pub fn sys_getdents64(
        &mut self,
        fd: FileDesc,
        dir_ptr: VirtAddr,
        len: usize,
    ) -> KResult<isize> {
        let current = current_task();
        let opened_files = current.opened_files.lock();
        let dir = opened_files.get(fd)?;
        let mut writer = UserBufferWriter::from_vaddr(dir_ptr, len);
        while let Some(entry) = dir.readdir()? {
            let alignment = size_of::<u64>();
            let record_len = align_up(
                size_of::<u64>() * 2 + size_of::<u16>() + 1 + entry.name.len() + 1,
                alignment,
            );
            if writer.remaining_len() + record_len > len {
                break;
            }

            writer.write(entry.inode_no as u64)?;
            writer.write(dir.pos() as u64)?;
            writer.write(record_len as u16)?;
            writer.write(entry.file_type as u8)?;
            writer.write_bytes(entry.name.as_bytes())?;
            writer.write(0u8)?;
            writer.skip_until_alignment(alignment)?;
        }

        Ok(writer.written_len() as isize)
    }

    pub fn sys_chdir(&mut self, path: &Path) -> KResult<isize> {
        current_task().root_fs.lock().chdir(path)?;
        Ok(0)
    }
}
