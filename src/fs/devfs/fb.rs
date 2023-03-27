use alloc::sync::Arc;
use spin::Once;

use crate::{fs::{FsNode, File, opened_file::OpenFlags, initramfs::get_root, INode}, util::KResult, userland::buffer::{UserBufferMut, UserBufferWriter, UserBuffer, UserBufferReader}, graphics::fb};

pub static DEV_FB0: Once<Arc<FbDevice>> = Once::new();

pub fn init() {
    let fb0 = Arc::new(FbDevice);
    get_root()
        .unwrap()
        .root_dir()
        .lookup("dev")
        .unwrap()
        .as_dir()
        .unwrap()
        .insert(INode::File(fb0.clone()));
    DEV_FB0.call_once(|| fb0);
}

pub struct FbDevice;

impl FsNode for FbDevice {
    fn get_name(&self) -> alloc::string::String {
        "fb0".into()
    }
}

impl File for FbDevice {
    fn read(&self, offset: usize, buf: UserBufferMut, _options: &OpenFlags) -> KResult<usize> {
        assert_eq!(offset % 4, 0);
        let buf_len = buf.len();
        assert_eq!(buf_len % 4, 0);
        let mut writer = UserBufferWriter::from(buf);
        let mut fb = fb();
        let mem = fb.frame_mut();
        let start = offset/4;
        let len = buf_len/4;
        let end = (start + len).min(mem.len());
        let mem = mem[start..end].iter().flat_map(|pixel| pixel.to_le_bytes());
        for byte in mem {
            writer.write(byte)?;
        }
        Ok((end - start) * 4)
    }

    fn write(&self, offset: usize, buf: UserBuffer<'_>, _options: &OpenFlags) -> KResult<usize> {
        assert_eq!(offset % 4, 0);
        let buf_len = buf.len();
        assert_eq!(buf_len % 4, 0);
        let mut reader = UserBufferReader::from(buf);
        let mut fb = fb();
        let mem = fb.frame_mut();
        let mut i = 0;
        while (offset / 4 + i) < mem.len() && i < buf_len / 4 {
            let byte0 = reader.read::<u8>()?;
            let byte1 = reader.read::<u8>()?;
            let byte2 = reader.read::<u8>()?;
            let byte3 = reader.read::<u8>()?;
            let pixel = u32::from_le_bytes([byte0, byte1, byte2, byte3]);
            mem[offset / 4 + i] = pixel;
            i += 1;
        }
        fb.present();
        Ok(i * 4)
    }
}