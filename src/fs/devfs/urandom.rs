use alloc::sync::Arc;
use x86::random::rdrand_slice;

use crate::{fs::{FsNode, File, initramfs::get_root, path::Path, INode}, userland::buffer::UserBufferWriter};

pub fn init() {
    get_root()
        .unwrap()
        .lookup(Path::new("dev"), true)
        .unwrap()
        .as_dir()
        .unwrap()
        .insert(INode::File(Arc::new(URandom)));
}

pub struct URandom;

impl FsNode for URandom {
    fn get_name(&self) -> alloc::string::String {
        "urandom".into()
    }
}

impl File for URandom {
    fn read(&self, _offset: usize, buf: crate::userland::buffer::UserBufferMut, _options: &crate::fs::opened_file::OpenFlags) -> crate::util::KResult<usize> {
        let mut bytes = alloc::vec![0u8; buf.len()];
        unsafe {
            rdrand_slice(&mut bytes);
        }
        let mut writer = UserBufferWriter::from(buf);
        writer.write_bytes(&bytes)?;
        Ok(writer.written_len())
    }
}