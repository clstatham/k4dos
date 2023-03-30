use alloc::{string::String, vec, vec::Vec};

use crate::{
    fs::{opened_file::OpenFlags, File, FileMode, FsNode, Stat, S_IFREG},
    userland::buffer::{UserBuffer, UserBufferMut, UserBufferReader, UserBufferWriter},
    util::{lock::IrqMutex, KResult},
};

pub struct InitRamFsFile {
    pub name: IrqMutex<String>,
    pub(super) data: IrqMutex<Vec<u8>>,
    pub(super) stat: IrqMutex<Stat>,
}

impl InitRamFsFile {
    pub fn new(name: String, inode_no: usize) -> InitRamFsFile {
        InitRamFsFile {
            name: IrqMutex::new(name),
            data: IrqMutex::new(Vec::new()),
            stat: IrqMutex::new(Stat {
                inode_no,
                mode: FileMode::new(S_IFREG | 0o644),
                ..Stat::zeroed()
            }),
        }
    }
}

impl FsNode for InitRamFsFile {
    fn get_name(&self) -> String {
        self.name.lock().clone()
    }
}

impl File for InitRamFsFile {
    fn read(&self, offset: usize, buf: UserBufferMut<'_>, _options: &OpenFlags) -> KResult<usize> {
        let lock = self.data.lock();
        if offset > lock.len() {
            return Ok(0);
        }
        let mut writer = UserBufferWriter::from(buf);
        writer.write_bytes(&lock[offset..])
    }

    fn write(&self, offset: usize, buf: UserBuffer<'_>, options: &OpenFlags) -> KResult<usize> {
        let mut reader = UserBufferReader::from(buf);
        let mut data = self.data.lock();
        let data_len = data.len();
        if data.is_empty() {
            data.extend_from_slice(&vec![0u8; offset + reader.remaining_len()]);
        } else if offset + reader.remaining_len() < isize::MAX as usize
            && (offset + reader.remaining_len() > data_len || options.contains(OpenFlags::O_APPEND))
        {
            data.push(0u8);
        }

        reader.read_bytes(&mut data[offset..])
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(*self.stat.lock())
    }
}
