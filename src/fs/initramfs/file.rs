use alloc::{string::String, vec::Vec};

use crate::{util::{lock::SpinLock, KResult}, fs::{File, FsNode, Stat, opened_file::OpenOptions, FileMode, S_IFREG}, errno, userland::buffer::{UserBufferMut, UserBufferWriter, UserBuffer, UserBufferReader}};

pub struct InitRamFsFile {
    pub name: SpinLock<String>,
    pub(super) data: SpinLock<Vec<u8>>,
    pub(super) stat: SpinLock<Stat>,
}

impl InitRamFsFile {
    pub fn new(name: String, inode_no: usize) -> InitRamFsFile {
        InitRamFsFile { name: SpinLock::new(name), data: SpinLock::new(Vec::new()), stat: SpinLock::new(Stat { inode_no, mode: FileMode::new(S_IFREG | 0o644), ..Stat::zeroed() }) }
    }
}

impl FsNode for InitRamFsFile {
    fn get_name(&self) -> String {
        self.name.lock().clone()
    }
}

impl File for InitRamFsFile {
    fn read(&self, offset: usize, buf: UserBufferMut<'_>, _options: &OpenOptions) -> KResult<usize> {
        let mut lock = self.data.lock();
        if offset > lock.len() {
            return Ok(0);
        }
        let mut writer = UserBufferWriter::from(buf);
        writer.write_bytes(&lock[offset..])
    }

    fn write(&self, offset: usize, buf: UserBuffer<'_>, _options: &OpenOptions) -> KResult<usize> {
        let mut reader = UserBufferReader::from(buf);
        reader.read_bytes(&mut self.data.lock()[offset..])
    }
}