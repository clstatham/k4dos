use core::{
    fmt::Debug,
    ops::{Add, AddAssign, Sub, SubAssign},
};

use crate::mem::{
    addr::{PhysAddr, VirtAddr},
    consts::PAGE_SIZE,
};

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

macro_rules! unit_impl {
    ($name:ident, $addr:ident) => {
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)]
        pub struct $name {
            index: PageIndex,
        }

        impl $name {
            #[inline]
            pub fn containing_address(addr: $addr) -> Self {
                Self {
                    index: PageIndex(addr.value() / PAGE_SIZE),
                }
            }

            #[inline]
            pub fn at_index(index: PageIndex) -> Self {
                Self { index }
            }

            #[inline]
            pub fn start_address(self) -> $addr {
                $addr::new(self.index.0 * PAGE_SIZE)
            }

            #[inline]
            pub fn inclusive_end_address(self) -> $addr {
                $addr::new(self.index.0 * PAGE_SIZE + PAGE_SIZE - 1)
            }

            #[inline]
            pub fn index(self) -> PageIndex {
                self.index
            }
        }

        impl Add<usize> for $name {
            type Output = Self;
            fn add(self, rhs: usize) -> Self::Output {
                Self::at_index(self.index + rhs)
            }
        }

        impl Sub<usize> for $name {
            type Output = Self;
            fn sub(self, rhs: usize) -> Self::Output {
                Self::at_index(self.index - rhs)
            }
        }

        impl AddAssign<usize> for $name {
            fn add_assign(&mut self, rhs: usize) {
                self.index += rhs
            }
        }

        impl SubAssign<usize> for $name {
            fn sub_assign(&mut self, rhs: usize) {
                self.index -= rhs
            }
        }
    };
}

unit_impl!(Frame, PhysAddr);
unit_impl!(Page, VirtAddr);

impl Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Frame<{:x}>", self.start_address())
    }
}

impl Debug for Page {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Page<{:x}>", self.start_address())
    }
}

#[derive(Debug)]
pub struct PageMergeError;

macro_rules! range_impl {
    ($name:ident, $addr:ident, $unit:ident) => {
        #[derive(Clone, Copy)]
        pub struct $name {
            start: $unit,
            end: $unit,
        }

        impl $name {
            #[inline]
            pub fn new(start: $unit, end: $unit) -> Self {
                Self { start, end }
            }

            #[inline]
            pub fn empty() -> Self {
                Self {
                    start: $unit::at_index(PageIndex(1)),
                    end: $unit::at_index(PageIndex(0)),
                }
            }

            #[inline]
            pub fn start(self) -> $unit {
                self.start
            }

            #[inline]
            pub fn end(self) -> $unit {
                self.end
            }

            #[inline]
            pub fn size_in_pages(self) -> usize {
                (self.start.index().0 - self.end.index().0) as usize
            }

            #[inline]
            pub fn size_in_bytes(self) -> usize {
                self.size_in_pages() * PAGE_SIZE as usize
            }

            #[inline]
            pub fn is_empty(self) -> bool {
                self.start > self.end
            }

            #[inline]
            pub fn start_address(self) -> $addr {
                self.start.start_address()
            }

            #[inline]
            pub fn inclusive_end_address(self) -> $addr {
                self.end.inclusive_end_address()
            }

            pub fn merge_with(
                &mut self,
                other: Self,
            ) -> $crate::util::error::KResult<(), PageMergeError> {
                if other.is_empty() {
                    return Ok(());
                }
                if other.start != self.end + 1 && other.end + 1 != self.start {
                    return Err($crate::kerr!(PageMergeError));
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

            pub fn contains(self, unit: $unit) -> bool {
                if self.is_empty() {
                    return false;
                }
                self.start <= unit && self.end >= unit
            }
        }
    };
}

range_impl!(FrameRange, PhysAddr, Frame);
range_impl!(PageRange, VirtAddr, Page);

impl Debug for FrameRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "FrameRange[{:x} ..= {:x}]",
            self.start_address(),
            self.inclusive_end_address()
        )
    }
}

impl Debug for PageRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "PageRange[{:x} ..= {:x}]",
            self.start_address(),
            self.inclusive_end_address()
        )
    }
}

macro_rules! allocated_impl {
    ($name:ident, $range:ident) => {
        pub struct $name {
            inner: $range,
        }

        impl $name {
            #[inline]
            pub unsafe fn assume_allocated(inner: $range) -> Self {
                Self { inner }
            }
        }

        impl core::ops::Deref for $name {
            type Target = $range;
            fn deref(&self) -> &Self::Target {
                &self.inner
            }
        }

        impl core::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.inner
            }
        }
    };
}

allocated_impl!(AllocatedFrames, FrameRange);
allocated_impl!(AllocatedPages, PageRange);

impl Debug for AllocatedFrames {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "AllocatedFrames[{:x} ..= {:x}]",
            self.start_address(),
            self.inclusive_end_address()
        )
    }
}

impl Debug for AllocatedPages {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "AllocatedPages[{:x} ..= {:x}]",
            self.start_address(),
            self.inclusive_end_address()
        )
    }
}

use x86_64::structures::paging::PageTableFlags;

macro_rules! mapped_impl {
    ($name:ident, $ap:ident) => {
        pub struct $name {
            pages: $ap,
            frames: AllocatedFrames,
            flags: PageTableFlags,
        }

        impl $name {
            #[inline]
            pub unsafe fn assume_mapped(
                pages: $ap,
                frames: AllocatedFrames,
                flags: PageTableFlags,
            ) -> Self {
                Self {
                    pages,
                    frames,
                    flags,
                }
            }

            #[inline]
            pub fn pages(&self) -> &$ap {
                &self.pages
            }

            #[inline]
            pub fn frames(&self) -> &AllocatedFrames {
                &self.frames
            }

            #[inline]
            pub fn flags(&self) -> PageTableFlags {
                self.flags
            }
        }
    };
}

mapped_impl!(MappedPages, AllocatedPages);
