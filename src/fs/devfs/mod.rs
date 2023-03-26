pub mod null;
pub mod tty;
pub mod urandom;

pub fn init() {
    self::tty::init();
    self::null::init();
    self::urandom::init();
}
