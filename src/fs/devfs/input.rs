//! TODO: make this more like an actual linux keyboard device file

use alloc::sync::Arc;
use pc_keyboard::{KeyEvent, KeyState};
use spin::{mutex::SpinMutex, Once};

use crate::{
    fs::{initramfs::get_root, opened_file::OpenFlags, File, FsNode, INode},
    userland::buffer::{UserBufferMut, UserBufferWriter},
    util::KResult,
};

pub static KBD_DEVICE: Once<Arc<KbdDevice>> = Once::new();

pub fn init() {
    let kbd = Arc::new(KbdDevice::new());
    get_root()
        .unwrap()
        .root_dir()
        .lookup("dev")
        .unwrap()
        .as_dir()
        .unwrap()
        .insert(INode::File(kbd.clone()));
    KBD_DEVICE.call_once(|| kbd);
}

pub struct KbdDevice {
    keys_pressed: Arc<SpinMutex<[u8; 256]>>,
}

impl KbdDevice {
    pub fn new() -> Self {
        Self {
            keys_pressed: Arc::new(SpinMutex::new([0u8; 256])),
        }
    }

    pub fn handle_kbd_irq(&self, key_evt: &KeyEvent) {
        match key_evt.state {
            KeyState::Down => self.keys_pressed.lock()[key_evt.code as u8 as usize] = 1,
            KeyState::Up => self.keys_pressed.lock()[key_evt.code as u8 as usize] = 0,
            _ => {}
        }
    }
}

impl Default for KbdDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl FsNode for KbdDevice {
    fn get_name(&self) -> alloc::string::String {
        "kbd".into()
    }
}

impl File for KbdDevice {
    fn read(&self, _offset: usize, buf: UserBufferMut, _options: &OpenFlags) -> KResult<usize> {
        let mut writer = UserBufferWriter::from(buf);
        let keys = *self.keys_pressed.lock();
        writer.write(keys)
    }
}
