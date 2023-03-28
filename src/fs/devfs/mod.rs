pub mod fb;
pub mod input;
pub mod null;
pub mod tty;
pub mod urandom;

pub fn init() {
    self::tty::init();
    self::null::init();
    self::urandom::init();
    self::fb::init();
    self::input::init();
}
