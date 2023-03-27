pub mod null;
pub mod tty;
pub mod urandom;
pub mod fb;

pub fn init() {
    self::tty::init();
    self::null::init();
    self::urandom::init();
    self::fb::init();
}
