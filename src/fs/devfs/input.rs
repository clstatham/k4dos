//! TODO: make this more like an actual linux keyboard device file

use alloc::sync::Arc;
use pc_keyboard::{KeyEvent, KeyState};
use spin::{mutex::SpinMutex, Once};

use crate::{fs::{FsNode, File, opened_file::OpenFlags, initramfs::get_root, INode}, userland::buffer::{UserBufferMut, UserBufferWriter}, util::KResult}; 

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

// #[repr(C)]
// #[allow(non_snake_case)]
// #[derive(Default)]
// pub struct KeysPressed {
//     Escape: u8,
//     Key1: u8,
//     Key2: u8,
//     Key3: u8,
//     Key4: u8,
//     Key5: u8,
//     Key6: u8,
//     Key7: u8,
//     Key8: u8,
//     Key9: u8,
//     Key0: u8,
//     OemMinus: u8,
//     OemPlus: u8,
//     Backspace: u8,
//     Tab: u8,
//     Q: u8,
//     W: u8,
//     E: u8,
//     R: u8,
//     T: u8,
//     Y: u8,
//     U: u8,
//     I: u8,
//     O: u8,
//     P: u8,
//     Oem4: u8,
//     Oem6: u8,
//     Return: u8,
//     LControl: u8,
//     A: u8,
//     S: u8,
//     D: u8,
//     F: u8,
//     G: u8,
//     H: u8,
//     J: u8,
//     K: u8,
//     L: u8,
//     Oem1: u8,
//     Oem3: u8,
//     Oem8: u8,
//     LShift: u8,
//     Oem7: u8,
//     Z: u8,
//     X: u8,
//     C: u8,
//     V: u8,
//     B: u8,
//     N: u8,
//     M: u8,
//     OemComma: u8,
//     OemPeriod: u8,
//     Oem2: u8,
//     RShift: u8,
//     NumpadMultiply: u8,
//     LAlt: u8,
//     Spacebar: u8,
//     CapsLock: u8,
//     F1: u8,
//     F2: u8,
//     F3: u8,
//     F4: u8,
//     F5: u8,
//     F6: u8,
//     F7: u8,
//     F8: u8,
//     F9: u8,
//     F10: u8,
//     NumpadLock: u8,
//     ScrollLock: u8,
//     Numpad7: u8,
//     Numpad8: u8,
//     Numpad9: u8,
//     NumpadSubtract: u8,
//     Numpad4: u8,
//     Numpad5: u8,
//     Numpad6: u8,
//     NumpadAdd: u8,
//     Numpad1: u8,
//     Numpad2: u8,
//     Numpad3: u8,
//     Numpad0: u8,
//     NumpadPeriod: u8,
//     SysRq: u8,
//     Oem5: u8,
//     F11: u8,
//     F12: u8,
// }