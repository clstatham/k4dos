use core::fmt::{self};
use core::mem::align_of;
use core::ops::*;
use core::ptr::NonNull;

use crate::kbail;
use crate::task::current_task;
use crate::util::{align_down, align_up, KResult};

use super::consts::PAGE_SIZE;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr {
    addr: usize,
}

#[inline]
pub const fn canonicalize_physaddr(addr: usize) -> usize {
    addr & 0x000F_FFFF_FFFF_FFFF
}

#[inline]
pub const fn is_canonical_physaddr(addr: usize) -> bool {
    canonicalize_physaddr(addr) == addr
}

#[inline]
pub const fn canonicalize_virtaddr(addr: usize) -> usize {
    ((addr << 16) as isize >> 16) as usize
}

#[inline]
pub const fn is_canonical_virtaddr(addr: usize) -> bool {
    canonicalize_virtaddr(addr) == addr
}

impl PhysAddr {
    #[inline]
    pub const unsafe fn new_unchecked(addr: usize) -> Self {
        Self { addr }
    }

    #[inline]
    pub const fn new(addr: usize) -> Self {
        assert!(
            is_canonical_physaddr(addr),
            "PhysAddr::new(): non-canonical address"
        );

        unsafe { Self::new_unchecked(addr) }
    }

    #[inline]
    pub const fn null() -> Self {
        unsafe { Self::new_unchecked(0) }
    }

    #[inline]
    pub const fn is_null(&self) -> bool {
        self.addr == 0
    }

    #[inline]
    pub const fn is_canonical(&self) -> bool {
        is_canonical_physaddr(self.addr)
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
        Self::new(self.addr + offset)
    }

    pub const fn const_sub(&self, offset: usize) -> Self {
        Self::new(self.addr - offset)
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
        // assert!(is_canonical_virtaddr(addr));
        unsafe { Self::new_unchecked(canonicalize_virtaddr(addr)) }
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
    pub const fn is_canonical(&self) -> bool {
        is_canonical_virtaddr(self.addr)
    }

    #[inline]
    pub const fn value(&self) -> usize {
        self.addr
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
    pub const fn as_ptr<T>(self) -> UnsafePtr<T> {
        UnsafePtr::new(self)
    }

    #[inline]
    pub fn as_hhdm_phys(&self) -> PhysAddr {
        PhysAddr::new(self.value() - crate::phys_offset().value())
    }

    pub const fn align_ok<T: Sized>(&self) -> KResult<()> {
        if self.addr == 0 {
            kbail!(EFAULT, "align_ok(): null VirtAddr");
        }
        if self.addr % align_of::<T>() != 0 {
            kbail!(EFAULT, "align_ok(): unaligned VirtAddr");
        }
        if !is_canonical_virtaddr(self.addr) {
            kbail!(EFAULT, "align_ok(): non-canonical VirtAddr");
        }

        Ok(())
    }

    pub fn user_ok(&self) -> KResult<()> {
        if self.addr == 0 {
            kbail!(EFAULT, "user_ok(): null VirtAddr");
        }
        if !is_canonical_virtaddr(self.addr) {
            kbail!(EFAULT, "user_ok(): non-canonical VirtAddr");
        }
        if self >= &crate::mem::consts::MAX_LOW_VADDR {
            kbail!(EFAULT, "user_ok(): VirtAddr in kernel memory");
        }

        Ok(())
    }

    pub unsafe fn read<T: Sized + Copy>(&self) -> KResult<T> {
        self.align_ok::<T>()?;
        Ok(unsafe { core::ptr::read(self.as_raw_ptr::<T>().cast()) })
    }

    pub unsafe fn read_volatile<T: Sized + Copy>(&self) -> KResult<T> {
        self.align_ok::<T>()?;
        Ok(unsafe { core::ptr::read_volatile(self.as_raw_ptr::<T>().cast()) })
    }

    pub unsafe fn read_user<T: Sized + Copy>(&self) -> KResult<T> {
        self.align_ok::<T>()?;
        self.user_ok()?;
        let _guard = current_task().arch_mut().address_space.temporarily_switch();
        Ok(unsafe { core::ptr::read_volatile(self.as_raw_ptr::<T>().cast()) })
    }

    pub unsafe fn read_bytes(&self, buf: &mut [u8]) -> KResult<usize> {
        self.align_ok::<u8>()?;
        unsafe { core::ptr::copy(self.as_raw_ptr(), buf.as_mut_ptr(), buf.len()) };
        Ok(buf.len())
    }

    pub unsafe fn read_bytes_user(&self, buf: &mut [u8]) -> KResult<usize> {
        self.align_ok::<u8>()?;
        (*self + buf.len()).user_ok()?;
        let _guard = current_task().arch_mut().address_space.temporarily_switch();
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

    pub unsafe fn write_user<T: Sized + Copy>(&self, t: T) -> KResult<()> {
        self.align_ok::<T>()?;
        (*self + size_of::<T>()).user_ok()?;
        let _guard = current_task().arch_mut().address_space.temporarily_switch();
        unsafe { core::ptr::write_volatile(self.as_raw_ptr_mut(), t) };
        Ok(())
    }

    pub unsafe fn write_bytes(&self, bytes: &[u8]) -> KResult<usize> {
        if self.is_null() {
            kbail!(EFAULT, "write_bytes(): null VirtAddr");
        }
        unsafe {
            core::slice::from_raw_parts_mut(self.as_raw_ptr_mut(), bytes.len())
                .copy_from_slice(bytes)
        };
        Ok(bytes.len())
    }

    pub unsafe fn write_bytes_user(&self, bytes: &[u8]) -> KResult<usize> {
        if self.is_null() {
            kbail!(EFAULT, "write_bytes_user(): null VirtAddr");
        }
        self.user_ok()?;
        (*self + bytes.len()).user_ok()?;
        let _guard = current_task().arch_mut().address_space.temporarily_switch();
        unsafe {
            core::slice::from_raw_parts_mut(self.as_raw_ptr_mut(), bytes.len())
                .copy_from_slice(bytes)
        };
        Ok(bytes.len())
    }

    pub unsafe fn fill(&self, value: u8, len: usize) -> KResult<usize> {
        if self.is_null() {
            kbail!(EFAULT, "fill(): null VirtAddr");
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

/// Like a `NonNull<T>`, but backed by a `VirtAddr`.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct UnsafePtr<T> {
    addr: VirtAddr,
    _phantom: core::marker::PhantomData<*mut T>,
}

impl<T> UnsafePtr<T> {
    pub const fn new(addr: VirtAddr) -> Self {
        assert!(!addr.is_null(), "UnsafePtr::new(): null VirtAddr");
        assert!(
            addr.value() % align_of::<T>() == 0,
            "UnsafePtr::new(): unaligned VirtAddr"
        );
        assert!(
            is_canonical_virtaddr(addr.value()),
            "UnsafePtr::new(): non-canonical VirtAddr"
        );

        Self {
            addr,
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn from_ref(data: &T) -> Self {
        Self::new(VirtAddr::new(data as *const T as usize))
    }

    pub fn from_mut(data: &mut T) -> Self {
        Self::new(VirtAddr::new(data as *mut T as usize))
    }

    pub fn from_raw_ptr(ptr: *mut T) -> Self {
        Self::new(VirtAddr::new(ptr as usize))
    }

    pub fn as_raw_ptr(&self) -> *const T {
        self.addr.as_raw_ptr()
    }

    pub fn as_raw_ptr_mut(&self) -> *mut T {
        self.addr.as_raw_ptr_mut()
    }

    pub unsafe fn deref(&self) -> &T {
        unsafe { &*self.as_raw_ptr() }
    }

    pub unsafe fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.as_raw_ptr_mut() }
    }

    pub unsafe fn as_ref(&self) -> Ref<'_, T> {
        Ref::new(unsafe { self.deref() })
    }

    pub unsafe fn as_mut(&mut self) -> Mut<'_, T> {
        Mut::new(unsafe { self.deref_mut() })
    }

    pub fn cast<U>(self) -> UnsafePtr<U> {
        UnsafePtr::new(self.addr)
    }

    pub fn into_non_null(self) -> NonNull<T> {
        assert!(
            !self.addr.is_null(),
            "UnsafePtr::as_non_null(): null VirtAddr"
        );
        NonNull::new(self.as_raw_ptr_mut()).unwrap()
    }

    pub fn read(&self) -> KResult<T>
    where
        T: Copy,
    {
        unsafe { self.addr.read() }
    }

    pub fn read_volatile(&self) -> KResult<T>
    where
        T: Copy,
    {
        unsafe { self.addr.read_volatile() }
    }
}

impl<T> fmt::Pointer for UnsafePtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_raw_ptr(), f)
    }
}

/// Like a `NonNull<[T]>`, but backed by a `VirtAddr`.
#[derive(Debug, Clone, Copy)]
pub struct UnsafeSlice<T> {
    addr: VirtAddr,
    len: usize,
    _phantom: core::marker::PhantomData<*mut [T]>,
}

impl<T> UnsafeSlice<T> {
    pub fn new(addr: VirtAddr, len: usize) -> Self {
        assert!(!addr.is_null(), "UnsafeSlice::new(): null VirtAddr");
        assert!(
            addr.value() % align_of::<T>() == 0,
            "UnsafeSlice::new(): unaligned VirtAddr"
        );
        assert!(
            is_canonical_virtaddr(addr.value()),
            "UnsafeSlice::new(): non-canonical VirtAddr"
        );

        Self {
            addr,
            len,
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> UnsafePtr<T> {
        UnsafePtr::new(self.addr)
    }

    pub unsafe fn as_slice(&self) -> &[T] {
        unsafe { core::slice::from_raw_parts(self.addr.as_raw_ptr(), self.len) }
    }

    pub unsafe fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { core::slice::from_raw_parts_mut(self.addr.as_raw_ptr_mut(), self.len) }
    }

    pub fn into_raw_parts(self) -> (VirtAddr, usize) {
        (self.addr, self.len)
    }
}

/// Like a `&T`, but backed by a `VirtAddr`.
#[derive(Debug, Clone, Copy)]
pub struct Ref<'a, T> {
    addr: VirtAddr,
    _phantom: core::marker::PhantomData<&'a T>,
}

impl<'a, T> Ref<'a, T> {
    pub fn new(data: &'a T) -> Self {
        Self {
            addr: VirtAddr::new(data as *const T as usize),
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn addr(&self) -> VirtAddr {
        self.addr
    }

    #[allow(clippy::should_implement_trait)]
    pub fn deref(&self) -> &'a T {
        unsafe { &*self.as_raw_ptr() }
    }

    pub fn as_ptr(&self) -> UnsafePtr<T> {
        self.addr.as_ptr()
    }

    pub fn as_raw_ptr(&self) -> *const T {
        self.addr.as_raw_ptr()
    }

    pub fn as_raw_ptr_mut(&self) -> *mut T {
        self.addr.as_raw_ptr_mut()
    }
}

impl<T> Deref for Ref<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        <Ref<T>>::deref(self)
    }
}

impl<T> fmt::Pointer for Ref<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_raw_ptr(), f)
    }
}

/// Like a `&mut T`, but backed by a `VirtAddr`.
#[derive(Debug)]
pub struct Mut<'a, T> {
    addr: VirtAddr,
    _phantom: core::marker::PhantomData<&'a mut T>,
}

impl<'a, T> Mut<'a, T> {
    pub fn new(data: &'a mut T) -> Self {
        Self {
            addr: VirtAddr::new(data as *mut T as usize),
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn addr(&self) -> VirtAddr {
        self.addr
    }

    pub fn as_ptr(&self) -> UnsafePtr<T> {
        self.addr.as_ptr()
    }

    #[allow(clippy::should_implement_trait)]
    pub fn deref(&self) -> &'a T {
        unsafe { &*self.as_raw_ptr() }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn deref_mut(&mut self) -> &'a mut T {
        unsafe { &mut *self.as_raw_ptr_mut() }
    }

    pub fn as_raw_ptr(&self) -> *const T {
        self.addr.as_raw_ptr()
    }

    pub fn as_raw_ptr_mut(&self) -> *mut T {
        self.addr.as_raw_ptr_mut()
    }
}

impl<T> fmt::Pointer for Mut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_raw_ptr(), f)
    }
}

impl<T> Deref for Mut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        <Mut<T>>::deref(self)
    }
}

impl<T> DerefMut for Mut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        <Mut<T>>::deref_mut(self)
    }
}

/// Like a `&[T]`, but backed by a `VirtAddr`.
#[derive(Debug, Clone, Copy)]
pub struct Slice<'a, T> {
    addr: VirtAddr,
    len: usize,
    _phantom: core::marker::PhantomData<&'a [T]>,
}

impl<'a, T> Slice<'a, T> {
    pub fn new(data: &'a [T]) -> Self {
        Self {
            addr: VirtAddr::new(data.as_ptr() as usize),
            len: data.len(),
            _phantom: core::marker::PhantomData,
        }
    }

    pub unsafe fn from_raw_parts(addr: VirtAddr, len: usize) -> Self {
        addr.align_ok::<T>().unwrap();
        Self {
            addr,
            len,
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn addr(&self) -> VirtAddr {
        self.addr
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> UnsafePtr<T> {
        self.addr.as_ptr()
    }

    pub fn as_slice(&self) -> &'a [T] {
        unsafe { core::slice::from_raw_parts(self.addr.as_raw_ptr(), self.len) }
    }

    pub fn as_raw_ptr(&self) -> *const T {
        self.addr.as_raw_ptr()
    }

    pub fn as_raw_ptr_mut(&self) -> *mut T {
        self.addr.as_raw_ptr_mut()
    }
}

impl<T> fmt::Pointer for Slice<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_raw_ptr(), f)
    }
}

impl<T> Deref for Slice<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> Index<usize> for Slice<'_, T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

/// Like a `&mut [T]`, but backed by a `VirtAddr`.
#[derive(Debug)]
pub struct SliceMut<'a, T> {
    addr: VirtAddr,
    len: usize,
    _phantom: core::marker::PhantomData<&'a mut [T]>,
}

impl<'a, T> SliceMut<'a, T> {
    pub fn new(data: &'a mut [T]) -> Self {
        Self {
            addr: VirtAddr::new(data.as_mut_ptr() as usize),
            len: data.len(),
            _phantom: core::marker::PhantomData,
        }
    }

    pub unsafe fn from_raw_parts(addr: VirtAddr, len: usize) -> Self {
        addr.align_ok::<T>().unwrap();
        Self {
            addr,
            len,
            _phantom: core::marker::PhantomData,
        }
    }

    pub fn addr(&self) -> VirtAddr {
        self.addr
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_ptr(&self) -> UnsafePtr<T> {
        self.addr.as_ptr()
    }

    pub fn as_slice(&self) -> &'a [T] {
        unsafe { core::slice::from_raw_parts(self.addr.as_raw_ptr(), self.len) }
    }

    pub fn as_mut_slice(&mut self) -> &'a mut [T] {
        unsafe { core::slice::from_raw_parts_mut(self.addr.as_raw_ptr_mut(), self.len) }
    }

    pub fn as_raw_ptr(&self) -> *const T {
        self.addr.as_raw_ptr()
    }

    pub fn as_raw_ptr_mut(&self) -> *mut T {
        self.addr.as_raw_ptr_mut()
    }
}

impl<T> fmt::Pointer for SliceMut<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_raw_ptr(), f)
    }
}

impl<T> Deref for SliceMut<'_, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T> DerefMut for SliceMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T> Index<usize> for SliceMut<'_, T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T> IndexMut<usize> for SliceMut<'_, T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.as_mut_slice()[index]
    }
}
