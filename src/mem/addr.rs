use bit_field::BitField;

use core::fmt::{self, *};
use core::mem::align_of;
use core::ops::*;

use crate::kerrmsg;
use crate::{
    util::{align_down, align_up, KResult},
};

use super::consts::PAGE_SIZE;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr {
    addr: usize,
}

#[inline]
pub const fn canonicalisze_physaddr(addr: usize) -> usize {
    addr & 0x000F_FFFF_FFFF_FFFF
}

#[inline]
pub const fn canonicalisze_virtaddr(addr: usize) -> usize {
    ((addr << 16) as isize >> 16) as usize
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VirtAddr {
    addr: usize,
}

impl PhysAddr {
    #[inline]
    pub const unsafe fn new_unchecked(addr: usize) -> Self {
        Self { addr }
    }

    #[inline]
    pub fn new(addr: usize) -> Self {
        assert_eq!(
            addr.get_bits(52..64),
            0,
            "Non canonical physical address provided"
        );
        unsafe { Self::new_unchecked(addr) }
    }

    #[inline]
    pub const fn null() -> Self {
        unsafe { Self::new_unchecked(0) }
    }

    #[inline]
    pub const fn value(&self) -> usize {
        self.addr
    }

    #[inline]
    pub fn as_hhdm_virt(&self) -> VirtAddr {
        VirtAddr::new(crate::phys_offset().value() + self.value())
    }

    #[inline]
    pub fn is_aligned(&self, align: usize) -> bool {
        self.addr % align == 0
    }
}

impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("PhysAddr")
            .field(&format_args!("{:#x}", self.addr))
            .finish()
    }
}

impl fmt::Binary for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Binary::fmt(&self.addr, f)
    }
}

impl fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::LowerHex::fmt(&self.addr, f)
    }
}

impl fmt::Octal for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Octal::fmt(&self.addr, f)
    }
}

impl fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::UpperHex::fmt(&self.addr, f)
    }
}

impl fmt::Pointer for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&(self.addr as *const ()), f)
    }
}

impl Add<usize> for PhysAddr {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        PhysAddr::new(self.addr + rhs)
    }
}

impl AddAssign<usize> for PhysAddr {
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}

impl Sub<usize> for PhysAddr {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        PhysAddr::new(self.addr.checked_sub(rhs).unwrap())
    }
}

impl SubAssign<usize> for PhysAddr {
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}

impl Sub<PhysAddr> for PhysAddr {
    type Output = usize;
    fn sub(self, rhs: PhysAddr) -> Self::Output {
        self.value().checked_sub(rhs.value()).unwrap()
    }
}

impl VirtAddr {
    #[inline]
    pub const fn new(addr: usize) -> Self {
        Self { addr }
    }

    #[inline]
    pub const fn null() -> Self {
        Self { addr: 0 }
    }

    #[inline]
    pub const fn value(&self) -> usize {
        self.addr
    }

    #[inline]
    pub fn as_ptr<T>(&self) -> *const T {
        self.value() as *const T
    }

    #[inline]
    pub fn as_mut_ptr<T>(&self) -> *mut T {
        self.value() as *mut T
    }

    #[inline]
    pub fn as_hhdm_phys(&self) -> PhysAddr {
        PhysAddr::new(self.value() - crate::phys_offset().value())
    }

    pub fn read_ok<T: Sized>(&self) -> KResult<()> {
        // let ptr = self.as_ptr::<T>();
        if self.addr == 0 {
            return Err(kerrmsg!("Attempt to read null VirtAddr"));
        }
        if self.addr % align_of::<T>() != 0 {
            return Err(kerrmsg!("Attempt to read unaligned VirtAddr"));
        }

        Ok(())
    }

    pub fn read<T: Sized>(&self) -> KResult<&T> {
        self.read_ok::<T>()?;
        Ok(unsafe { &*(self.as_ptr()) })
    }

    pub fn read_mut<T: Sized>(&self) -> KResult<&mut T> {
        self.read_ok::<T>()?;
        Ok(unsafe { &mut *(self.as_mut_ptr()) })
    }

    pub fn read_bytes(&self, buf: &mut [u8]) -> KResult<usize> {
        self.read_ok::<u8>()?;
        unsafe { core::ptr::copy(self.as_ptr(), buf.as_mut_ptr(), buf.len()) };
        Ok(buf.len())
    }

    pub fn write<T: Sized>(&self, t: T) -> KResult<()> {
        if self.addr == 0 {
            return Err(kerrmsg!("Attempt to write to null VirtAddr"))
        }
        if self.addr % align_of::<T>() != 0 {
            return Err(kerrmsg!("Attempt to write to unaligned VirtAddr"))
        }
        unsafe { core::ptr::write(self.as_mut_ptr(), t) };
        Ok(())
    }

    pub fn write_bytes(&self, bytes: &[u8]) -> KResult<usize> {
        if self.addr == 0 {
            return Err(kerrmsg!("Attempt to write to null VirtAddr"))
        }
        unsafe { core::slice::from_raw_parts_mut(self.as_mut_ptr(), bytes.len()).copy_from_slice(bytes) };
        Ok(bytes.len())
    }

    pub fn fill(&mut self, value: u8, len: usize) -> KResult<usize> {
        if self.addr == 0 {
            return Err(kerrmsg!("Attempt to write to null VirtAddr"))
        }
        unsafe { (self.value() as *mut u8).write_bytes(value, len) };
        Ok(len)
    }

    #[inline]
    pub fn align_down<U>(&self, align: U) -> Self
    where
        U: Into<usize>,
    {
        VirtAddr::new(align_down(self.addr, align.into()))
    }

    #[inline]
    pub fn align_up<U>(&self, align: U) -> Self
    where
        U: Into<usize>,
    {
        VirtAddr::new(align_up(self.addr, align.into()))
    }

    pub fn p4_index(&self) -> usize {
        ((self.addr / PAGE_SIZE) >> 27) & 0x1FF
    }
    pub fn p3_index(&self) -> usize {
        ((self.addr / PAGE_SIZE) >> 18) & 0x1FF
    }
    pub fn p2_index(&self) -> usize {
        ((self.addr / PAGE_SIZE) >> 9) & 0x1FF
    }
    pub fn p1_index(&self) -> usize {
        (self.addr / PAGE_SIZE) & 0x1FF
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_tuple("VirtAddr")
            .field(&format_args!("{:#x}", self.addr))
            .finish()
    }
}

impl fmt::Binary for VirtAddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Binary::fmt(&self.addr, f)
    }
}

impl fmt::LowerHex for VirtAddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::LowerHex::fmt(&self.addr, f)
    }
}

impl fmt::Octal for VirtAddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Octal::fmt(&self.addr, f)
    }
}

impl fmt::UpperHex for VirtAddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::UpperHex::fmt(&self.addr, f)
    }
}

impl fmt::Pointer for VirtAddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&(self.addr as *const ()), f)
    }
}

impl Add<usize> for VirtAddr {
    type Output = Self;
    #[inline]
    fn add(self, rhs: usize) -> Self::Output {
        VirtAddr::new(self.addr + rhs)
    }
}

impl AddAssign<usize> for VirtAddr {
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}

impl Sub<usize> for VirtAddr {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: usize) -> Self::Output {
        VirtAddr::new(self.addr.checked_sub(rhs).unwrap())
    }
}

impl SubAssign<usize> for VirtAddr {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}

impl Sub<VirtAddr> for VirtAddr {
    type Output = usize;
    #[inline]
    fn sub(self, rhs: VirtAddr) -> Self::Output {
        self.value().checked_sub(rhs.value()).unwrap()
    }
}
