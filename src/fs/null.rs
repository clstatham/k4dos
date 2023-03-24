use alloc::{borrow::ToOwned, sync::Arc};
use spin::Once;

use crate::userland::buffer::UserBufferWriter;

use super::{initramfs::get_root, File, FsNode, INode, S_IFCHR};

static DEV_NULL: Once<Arc<NullDevice>> = Once::new();

pub fn init() {
    let null = Arc::new(NullDevice);
    get_root()
        .unwrap()
        .root_dir()
        .lookup("dev")
        .unwrap()
        .as_dir()
        .unwrap()
        .insert(INode::File(null.clone()));
    DEV_NULL.call_once(|| null);
}

pub struct NullDevice;

impl FsNode for NullDevice {
    fn get_name(&self) -> alloc::string::String {
        "null".to_owned()
    }
}

impl File for NullDevice {
    fn write(
        &self,
        _offset: usize,
        buf: crate::userland::buffer::UserBuffer<'_>,
        _options: &super::opened_file::OpenOptions,
    ) -> crate::util::KResult<usize> {
        Ok(buf.len())
    }

    fn read(
        &self,
        _offset: usize,
        buf: crate::userland::buffer::UserBufferMut,
        _options: &super::opened_file::OpenOptions,
        // len: usize,
    ) -> crate::util::KResult<usize> {
        let mut writer = UserBufferWriter::from(buf);
        writer.write_bytes(&[0x05])?; // EOF
        Ok(writer.written_len())
    }

    fn stat(&self) -> crate::util::KResult<super::Stat> {
        Ok(super::Stat {
            inode_no: 4,
            mode: super::FileMode(S_IFCHR),
            ..super::Stat::zeroed()
        })
    }
}
