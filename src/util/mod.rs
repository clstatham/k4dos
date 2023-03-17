#[macro_use]
pub mod error;
pub mod lock;

pub use self::error::*;
pub use self::lock::*;

#[inline]
pub const fn align_down(val: usize, align: usize) -> usize {
    val / align * align
}
#[inline]
pub const fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) / align * align
}
