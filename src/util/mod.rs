#[macro_use]
pub mod error;

pub use self::error::*;

#[inline]
pub const fn align_down(val: u64, align: u64) -> u64 {
    val / align * align
}
#[inline]
pub const fn align_up(val: u64, align: u64) -> u64 {
    (val + align - 1) / align * align
}
