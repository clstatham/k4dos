use core::fmt::{self};
use core::mem::align_of;
use core::ops::*;
use core::ptr::NonNull;

use crate::errno;
use crate::util::errno::Errno;
use crate::util::{align_down, align_up, KResult};

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
pub const fn is_canonical_physaddr(addr: usize) -> bool {
    canonicalisze_physaddr(addr) == addr
}

#[inline]
pub const fn canonicalisze_virtaddr(addr: usize) -> usize {
    ((addr << 16) as isize >> 16) as usize
}

#[inline]
pub const fn is_canonical_virtaddr(addr: usize) -> bool {
    canonicalisze_virtaddr(addr) == addr
}

impl PhysAddr {
    #[inline]
    pub const unsafe fn new_unchecked(addr: usize) -> Self {
        Self { addr }
    }

    #[inline]
    pub const fn new(addr: usize) -> Option<Self> {
        if !is_canonical_physaddr(addr) {
            return None;
        }

        Some(unsafe { Self::new_unchecked(addr) })
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

    pub const fn const_add(&self, offset: usize) -> Self {
        Self::new(self.addr + offset).unwrap()
    }

    pub const fn const_sub(&self, offset: usize) -> Self {
        Self::new(self.addr - offset).unwrap()
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
        PhysAddr::new(self.addr + rhs).unwrap()
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
        PhysAddr::new(self.addr.checked_sub(rhs).unwrap()).unwrap()
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VirtAddr {
    addr: usize,
}

impl VirtAddr {
    #[inline]
    pub const unsafe fn new_unchecked(addr: usize) -> Self {
        Self { addr }
    }

    #[inline]
    pub const fn new(addr: usize) -> Self {
        assert!(is_canonical_virtaddr(addr));

        unsafe { Self::new_unchecked(addr) }
    }

    #[inline]
    pub const fn null() -> Self {
        Self { addr: 0 }
    }

    #[inline]
    pub const fn is_null(&self) -> bool {
        self.addr == 0
    }

    #[inline]
    pub const fn value(&self) -> usize {
        self.addr
    }

    #[inline]
    pub const unsafe fn alias(&self) -> Self {
        unsafe { Self::new_unchecked(self.addr) }
    }

    #[inline]
    pub const fn const_add(&self, offset: usize) -> Self {
        Self::new(self.addr + offset)
    }

    #[inline]
    pub const fn const_sub(&self, offset: usize) -> Self {
        Self::new(self.addr - offset)
    }

    #[inline]
    pub const fn as_raw_ptr<T>(&self) -> *const T {
        self.value() as *const T
    }

    #[inline]
    pub const fn as_raw_ptr_mut<T>(&self) -> *mut T {
        self.value() as *mut T
    }

    #[inline]
    pub unsafe fn deref<T>(&self) -> KResult<&T> {
        self.align_ok::<T>()?;
        Ok(unsafe { &*self.as_raw_ptr() })
    }

    #[inline]
    pub unsafe fn deref_mut<T>(&mut self) -> KResult<&mut T> {
        self.align_ok::<T>()?;
        Ok(unsafe { &mut *self.as_raw_ptr_mut() })
    }

    #[inline]
    pub const fn into_ptr<T>(self) -> Ptr<T> {
        Ptr::new(self)
    }

    #[inline]
    pub fn as_hhdm_phys(&self) -> PhysAddr {
        PhysAddr::new(self.value() - crate::phys_offset().value()).unwrap()
    }

    pub const fn align_ok<T: Sized>(&self) -> KResult<()> {
        if self.addr == 0 {
            return Err(errno!(Errno::EFAULT, "align_ok(): null VirtAddr"));
        }
        if self.addr % align_of::<T>() != 0 {
            return Err(errno!(Errno::EACCES, "align_ok(): unaligned VirtAddr"));
        }

        Ok(())
    }

    pub unsafe fn read<T: Sized + Copy>(&self) -> KResult<T> {
        self.align_ok::<T>()?;
        Ok(unsafe { core::ptr::read(self.as_raw_ptr::<T>().cast()) })
    }

    pub unsafe fn read_volatile<T: Sized + Copy>(&self) -> KResult<T> {
        self.align_ok::<T>()?;
        Ok(unsafe { self.as_raw_ptr::<T>().read_volatile() })
    }

    pub unsafe fn read_bytes(&self, buf: &mut [u8]) -> KResult<usize> {
        self.align_ok::<u8>()?;
        unsafe { core::ptr::copy(self.as_raw_ptr(), buf.as_mut_ptr(), buf.len()) };
        Ok(buf.len())
    }

    pub unsafe fn write<T: Sized + Copy>(&self, t: T) -> KResult<()> {
        self.align_ok::<T>()?;
        unsafe { core::ptr::write(self.as_raw_ptr_mut(), t) };
        Ok(())
    }

    pub unsafe fn write_volatile<T: Sized + Copy>(&self, t: T) -> KResult<()> {
        self.align_ok::<T>()?;
        unsafe { core::ptr::write_volatile(self.as_raw_ptr_mut(), t) };
        Ok(())
    }

    pub unsafe fn write_bytes(&self, bytes: &[u8]) -> KResult<usize> {
        if self.is_null() {
            return Err(errno!(Errno::EFAULT, "write_bytes(): null VirtAddr"));
        }
        unsafe {
            core::slice::from_raw_parts_mut(self.as_raw_ptr_mut(), bytes.len())
                .copy_from_slice(bytes)
        };
        Ok(bytes.len())
    }

    pub unsafe fn fill(&self, value: u8, len: usize) -> KResult<usize> {
        if self.is_null() {
            return Err(errno!(Errno::EFAULT, "fill(): null VirtAddr"));
        }
        unsafe { (self.value() as *mut u8).write_bytes(value, len) };
        Ok(len)
    }

    #[inline]
    pub const fn align_down(&self, align: usize) -> Self {
        VirtAddr::new(align_down(self.addr, align))
    }

    #[inline]
    pub const fn align_up(&self, align: usize) -> Self {
        VirtAddr::new(align_up(self.addr, align))
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
        self.addr += rhs;
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
        self.addr -= rhs;
    }
}

impl Sub<VirtAddr> for VirtAddr {
    type Output = usize;
    #[inline]
    fn sub(self, rhs: VirtAddr) -> Self::Output {
        self.value().checked_sub(rhs.value()).unwrap()
    }
}

pub struct Ptr<T> {
    ptr: VirtAddr,
    _phantom: core::marker::PhantomData<*mut T>,
}

impl<T> Ptr<T> {
    pub const fn new(ptr: VirtAddr) -> Self {
        assert!(!ptr.is_null(), "Ptr::new(): null VirtAddr");
        assert!(
            ptr.value() % align_of::<T>() == 0,
            "Ptr::new(): unaligned VirtAddr"
        );

        Self {
            ptr,
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn from_ptr(ptr: *const T) -> Self {
        Self::new(VirtAddr::new(ptr as usize))
    }

    pub fn as_raw_ptr(&self) -> *const T {
        self.ptr.as_raw_ptr()
    }

    pub fn as_raw_ptr_mut(&mut self) -> *mut T {
        self.ptr.as_raw_ptr_mut()
    }

    pub unsafe fn deref(&self) -> &T {
        unsafe { &*self.as_raw_ptr() }
    }

    pub unsafe fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.as_raw_ptr_mut() }
    }

    pub fn into_non_null(mut self) -> NonNull<T> {
        assert!(!self.ptr.is_null(), "Ptr::as_non_null(): null VirtAddr");
        NonNull::new(self.as_raw_ptr_mut()).unwrap()
    }

    pub fn read(&self) -> KResult<T>
    where
        T: Copy,
    {
        unsafe { self.ptr.read() }
    }

    pub fn read_volatile(&self) -> KResult<T>
    where
        T: Copy,
    {
        unsafe { self.ptr.read_volatile() }
    }
}
