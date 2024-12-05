use alloc::sync::Arc;
use spin::Once;

use crate::{
    fs::{initramfs::get_root, opened_file::OpenFlags, File, FsNode, INode},
    graphics::fb,
    kerror,
    mem::addr::VirtAddr,
    userland::buffer::{UserBuffer, UserBufferMut, UserBufferReader, UserBufferWriter},
    util::KResult,
};

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
        let start = offset / 4;
        let len = buf_len / 4;
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
            let pixel = reader.read::<u32>()?;
            // let pixel = u32::from_le_bytes([byte0, byte1, byte2, byte3]);
            mem[offset / 4 + i] = pixel;
            i += 1;
        }
        fb.present();
        Ok(i * 4)
    }

    fn ioctl(&self, cmd: usize, arg: usize) -> KResult<isize> {
        const FBIOGET_VSCREENINFO: usize = 0x4600;
        // const FBIOGET_FSCREENINFO: usize = 0x4602;
        match cmd {
            FBIOGET_VSCREENINFO => {
                let fb = fb();
                let info = FbVarScreenInfo {
                    xres: fb.width() as u32,
                    yres: fb.height() as u32,
                    xres_virtual: fb.width() as u32,
                    yres_virtual: fb.height() as u32,
                    bpp: fb.bpp() as u32,
                    ..FbVarScreenInfo::default()
                };
                unsafe { VirtAddr::new(arg).write_volatile(info) }?;
            }
            // FBIOGET_FSCREENINFO => {

            // }
            _ => return Err(kerror!(EINVAL, "ioctl(): unknown cmd")),
        }
        Ok(0)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbBitField {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FbVarScreenInfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bpp: u32,
    grayscale: u32,
    red: FbBitField,
    green: FbBitField,
    blue: FbBitField,
    transp: FbBitField,
    nonstd: u32,
    activate: u32,
    height_mm: u32,
    width_mm: u32,
    accel_flags: u32,
    pixclock: u32,
    left_margin: u32,
    right_margin: u32,
    upper_margin: u32,
    lower_margin: u32,
    hsync_len: u32,
    vsync_len: u32,
    sync: u32,
    vmode: u32,
    rotate: u32,
    colorspace: u32,
    reserved: [u32; 4],
}
