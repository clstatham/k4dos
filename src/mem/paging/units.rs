use core::{
    fmt::Debug,
    ops::{Add, AddAssign, Sub, SubAssign},
};

use x86_64::structures::paging::PageTableFlags;

use crate::mem::{
    addr::{PhysAddr, VirtAddr},
    consts::PAGE_SIZE,
};

use super::mapper::Mapper;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PageIndex(pub usize);

impl PageIndex {
    #[inline]
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    #[inline]
    pub fn containing_physaddr(paddr: PhysAddr) -> Self {
        Self::new(paddr.value() / PAGE_SIZE)
    }

    #[inline]
    pub fn containing_virtaddr(vaddr: VirtAddr) -> Self {
        Self::new(vaddr.value() / PAGE_SIZE)
    }

    #[inline]
    pub fn as_physaddr(self) -> PhysAddr {
        PhysAddr::new(self.0 * PAGE_SIZE)
    }

    #[inline]
    pub fn as_virtaddr(self) -> VirtAddr {
        VirtAddr::new(self.0 * PAGE_SIZE)
    }
}

impl Add<usize> for PageIndex {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl Sub<usize> for PageIndex {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl AddAssign<usize> for PageIndex {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs
    }
}

impl SubAssign<usize> for PageIndex {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs
    }
}

pub trait MemoryUnit:
    Copy
    + Debug
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + Add<usize, Output = Self>
    + Sub<usize, Output = Self>
{
    type Address: Copy
        + Debug
        + PartialEq
        + Eq
        + PartialOrd
        + Ord
        + Add<usize, Output = Self::Address>
        + Sub<usize, Output = Self::Address>
        + core::fmt::Pointer
        + core::fmt::LowerHex;

    fn containing_address(addr: Self::Address) -> Self;
    fn at_index(index: PageIndex) -> Self;
    fn start_address(self) -> Self::Address;
    fn inclusive_end_address(self) -> Self::Address;
    fn index(self) -> PageIndex;
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Frame {
    index: PageIndex,
}

impl MemoryUnit for Frame {
    type Address = PhysAddr;

    #[inline]
    fn containing_address(addr: PhysAddr) -> Self {
        Self {
            index: PageIndex::containing_physaddr(addr),
        }
    }

    #[inline]
    fn at_index(index: PageIndex) -> Self {
        Self { index }
    }

    #[inline]
    fn start_address(self) -> Self::Address {
        self.index.as_physaddr()
    }

    #[inline]
    fn inclusive_end_address(self) -> Self::Address {
        self.start_address() + PAGE_SIZE - 1
    }

    #[inline]
    fn index(self) -> PageIndex {
        self.index
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Frame<{:x}>", self.start_address())
    }
}

impl Add<usize> for Frame {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self::at_index(self.index + rhs)
    }
}

impl Sub<usize> for Frame {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        Self::at_index(self.index - rhs)
    }
}

impl AddAssign<usize> for Frame {
    fn add_assign(&mut self, rhs: usize) {
        self.index += rhs
    }
}

impl SubAssign<usize> for Frame {
    fn sub_assign(&mut self, rhs: usize) {
        self.index -= rhs
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Page {
    index: PageIndex,
}

impl MemoryUnit for Page {
    type Address = VirtAddr;

    #[inline]
    fn containing_address(addr: VirtAddr) -> Self {
        Self {
            index: PageIndex::containing_virtaddr(addr),
        }
    }

    #[inline]
    fn at_index(index: PageIndex) -> Self {
        Self { index }
    }

    #[inline]
    fn start_address(self) -> Self::Address {
        self.index.as_virtaddr()
    }

    #[inline]
    fn inclusive_end_address(self) -> Self::Address {
        self.index.as_virtaddr() + PAGE_SIZE - 1
    }

    #[inline]
    fn index(self) -> PageIndex {
        self.index
    }
}

impl Debug for Page {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Page<{:x}>", self.start_address())
    }
}

impl Add<usize> for Page {
    type Output = Self;
    fn add(self, rhs: usize) -> Self::Output {
        Self::at_index(self.index + rhs)
    }
}

impl Sub<usize> for Page {
    type Output = Self;
    fn sub(self, rhs: usize) -> Self::Output {
        Self::at_index(self.index - rhs)
    }
}

impl AddAssign<usize> for Page {
    fn add_assign(&mut self, rhs: usize) {
        self.index += rhs
    }
}

impl SubAssign<usize> for Page {
    fn sub_assign(&mut self, rhs: usize) {
        self.index -= rhs
    }
}

#[derive(Debug)]
pub struct PageMergeError;

#[derive(Debug, Clone, Copy)]
pub struct MemoryRange<T: MemoryUnit> {
    pub start: T,
    pub end: T,
}

impl<T: MemoryUnit> MemoryRange<T> {
    #[inline]
    pub fn new(start: T, end: T) -> Self {
        Self { start, end }
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            start: T::at_index(PageIndex(0)),
            end: T::at_index(PageIndex(0)),
        }
    }

    #[inline]
    pub fn start(self) -> T {
        self.start
    }

    #[inline]
    pub fn end(self) -> T {
        self.end
    }

    #[inline]
    pub fn size_in_pages(self) -> usize {
        if self.is_empty() {
            return 0;
        }
        self.end.index().0 - self.start.index().0
    }

    #[inline]
    pub fn size_in_bytes(self) -> usize {
        self.size_in_pages() * PAGE_SIZE
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }

    #[inline]
    pub fn start_address(self) -> T::Address {
        self.start.start_address()
    }

    #[inline]
    pub fn end_address(self) -> T::Address {
        self.end.start_address()
    }

    #[inline]
    pub fn inclusive_end_address(self) -> T::Address {
        self.end_address() - 1
    }

    pub fn merge_with(&mut self, other: Self) -> Result<(), PageMergeError> {
        if other.is_empty() {
            return Ok(());
        }
        if other.start != self.end && other.end != self.start {
            return Err(PageMergeError);
        }
        if other.start < self.start {
            self.start = other.start;
        }
        if other.end > self.end {
            self.end = other.end;
        }

        Ok(())
    }

    pub fn overlaps(self, other: Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.start <= other.end && other.start <= self.end
    }

    pub fn consumes(self, other: Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.start <= other.start && self.end >= other.end
    }

    pub fn contains(self, unit: T) -> bool {
        if self.is_empty() {
            return false;
        }
        self.start <= unit && self.end >= unit
    }

    pub fn iter(self) -> MemoryRangeIter<T> {
        MemoryRangeIter {
            current: self.start,
            limit: self.end,
        }
    }
}

pub type FrameRange = MemoryRange<Frame>;
pub type PageRange = MemoryRange<Page>;

pub struct MemoryRangeIter<T: MemoryUnit> {
    current: T,
    limit: T,
}

impl<T: MemoryUnit> Iterator for MemoryRangeIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.limit {
            None
        } else {
            let current = self.current;
            self.current = self.current + 1;
            Some(current)
        }
    }
}

pub struct Allocated<T: MemoryUnit> {
    pub range: MemoryRange<T>,
}

impl<T: MemoryUnit> Allocated<T> {
    pub unsafe fn assume_allocated(range: MemoryRange<T>) -> Self {
        Self { range }
    }
}

impl<T: MemoryUnit> Debug for Allocated<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Allocated[{:x} .. {:x}]",
            self.range.start_address(),
            self.range.inclusive_end_address()
        )
    }
}

pub type AllocatedFrames = Allocated<Frame>;
pub type AllocatedPages = Allocated<Page>;

impl<T: MemoryUnit> core::ops::Deref for Allocated<T> {
    type Target = MemoryRange<T>;
    fn deref(&self) -> &Self::Target {
        &self.range
    }
}

impl<T: MemoryUnit> core::ops::DerefMut for Allocated<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.range
    }
}

pub struct MappedPages {
    pub(super) pages: AllocatedPages,
    pub(super) frames: AllocatedFrames,
    pub(super) flags: PageTableFlags,
}

impl MappedPages {
    pub unsafe fn assume_mapped(
        pages: AllocatedPages,
        frames: AllocatedFrames,
        flags: PageTableFlags,
    ) -> Self {
        Self {
            pages,
            frames,
            flags,
        }
    }

    pub unsafe fn unmap(self, table: &mut Mapper) -> (AllocatedPages, AllocatedFrames) {
        unsafe { table.unmap(self) }
    }

    pub fn pages(&self) -> &AllocatedPages {
        &self.pages
    }

    pub fn frames(&self) -> &AllocatedFrames {
        &self.frames
    }

    pub fn flags(&self) -> PageTableFlags {
        self.flags
    }
}

impl Debug for MappedPages {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MappedPages")
            .field("pages", &self.pages)
            .field("frames", &self.frames)
            .finish()
    }
}
