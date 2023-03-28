use core::ops::{Index, IndexMut};

use alloc::vec::Vec;
use arrayvec::ArrayVec;
use buddy_system_allocator::LockedHeap;
use limine::{LimineMemmapEntry, LimineMemoryMapEntryType, NonNullPtr};
use spin::Once;

use super::addr::{PhysAddr, VirtAddr};
use super::consts::{MAX_LOW_VADDR, MIN_HIGH_VADDR, PAGE_SIZE};
use super::paging::units::{
    AllocatedFrames, AllocatedPages, Frame, FrameRange, Page, PageIndex, PageRange,
};
use crate::kerrmsg;
use crate::util::{align_down, IrqMutex, KResult};

pub static KERNEL_FRAME_ALLOCATOR: Once<IrqMutex<FrameAllocator>> = Once::new();
pub static KERNEL_PAGE_ALLOCATOR: Once<IrqMutex<PageAllocator>> = Once::new();

#[global_allocator]
pub static GLOBAL_ALLOC: LockedHeap<32> = LockedHeap::new();

pub fn alloc_kernel_frames(count: usize) -> KResult<AllocatedFrames> {
    KERNEL_FRAME_ALLOCATOR
        .get()
        .ok_or(kerrmsg!("KERNEL_FRAME_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate(count)
}

pub fn alloc_kernel_frames_at(start: Frame, count: usize) -> KResult<AllocatedFrames> {
    KERNEL_FRAME_ALLOCATOR
        .get()
        .ok_or(kerrmsg!("KERNEL_FRAME_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate_at(start, count)
}

pub fn alloc_kernel_pages(count: usize) -> KResult<AllocatedPages> {
    KERNEL_PAGE_ALLOCATOR
        .get()
        .ok_or(kerrmsg!("KERNEL_PAGE_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate(count)
}

pub fn alloc_kernel_pages_at(start: Page, count: usize) -> KResult<AllocatedPages> {
    KERNEL_PAGE_ALLOCATOR
        .get()
        .ok_or(kerrmsg!("KERNEL_PAGE_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate_at(start, count)
}

pub fn free_kernel_frames(frames: &mut AllocatedFrames, merge: bool) -> KResult<()> {
    KERNEL_FRAME_ALLOCATOR
        .get()
        .ok_or(kerrmsg!("KERNEL_FRAME_ALLOCATOR not initialized"))?
        .try_lock()?
        .free(frames, merge);
    Ok(())
}

pub fn free_kernel_pages(pages: &mut AllocatedPages, merge: bool) -> KResult<()> {
    KERNEL_PAGE_ALLOCATOR
        .get()
        .ok_or(kerrmsg!("KERNEL_PAGE_ALLOCATOR not initialized"))?
        .try_lock()?
        .free(pages, merge);
    Ok(())
}

pub fn init(memmap: &mut [NonNullPtr<LimineMemmapEntry>]) -> KResult<()> {
    let mut frame_alloc = FrameAllocator::new_static();
    for entry in memmap
        .iter()
        .filter(|entry| entry.typ == LimineMemoryMapEntryType::Usable)
    {
        let entry = unsafe { &*entry.as_ptr() };
        if (entry.len as usize) < PAGE_SIZE {
            continue;
        }
        let start = entry.base as usize;
        let end = start + entry.len as usize;
        let frames = FrameRange::new(
            Frame::containing_address(PhysAddr::new(start)),
            Frame::containing_address(PhysAddr::new(end)),
        );
        unsafe { frame_alloc.insert_free_region(frames) };
    }
    KERNEL_FRAME_ALLOCATOR.call_once(|| IrqMutex::new(frame_alloc));

    let mut page_alloc = PageAllocator::new_static();
    let pages = PageRange::new(
        Page::at_index(PageIndex(1)),
        Page::containing_address(VirtAddr::new(MAX_LOW_VADDR) - 1),
    );
    unsafe { page_alloc.insert_free_region(pages) };
    let pages = PageRange::new(
        Page::containing_address(VirtAddr::new(MIN_HIGH_VADDR)),
        Page::containing_address(VirtAddr::new(align_down(usize::MAX, PAGE_SIZE))),
    );
    unsafe { page_alloc.insert_free_region(pages) };
    KERNEL_PAGE_ALLOCATOR.call_once(|| IrqMutex::new(page_alloc));

    Ok(())
}

#[derive(Clone)]
pub enum StaticListOrVec<T: Clone, const STATIC_CAP: usize> {
    StaticList(ArrayVec<T, STATIC_CAP>),
    Vec(Vec<T>),
}

impl<T: Clone, const STATIC_CAP: usize> StaticListOrVec<T, STATIC_CAP> {
    pub const fn new_static() -> StaticListOrVec<T, STATIC_CAP> {
        StaticListOrVec::StaticList(ArrayVec::<T, STATIC_CAP>::new_const())
    }
    pub const fn new_vec() -> StaticListOrVec<T, STATIC_CAP> {
        StaticListOrVec::Vec(Vec::new())
    }
    pub fn push(&mut self, item: T) {
        match self {
            StaticListOrVec::StaticList(a) => a.push(item),
            StaticListOrVec::Vec(v) => v.push(item),
        }
    }
    pub fn remove(&mut self, index: usize) -> T {
        match self {
            StaticListOrVec::StaticList(a) => a.remove(index),
            StaticListOrVec::Vec(v) => v.remove(index),
        }
    }
    pub fn convert_to_vec(&mut self) {
        match self {
            StaticListOrVec::Vec(_v) => {}
            StaticListOrVec::StaticList(a) => {
                *self = StaticListOrVec::Vec(a.into_iter().map(|t| t.clone()).collect());
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        match self {
            StaticListOrVec::StaticList(a) => a.iter(),
            StaticListOrVec::Vec(v) => v.iter(),
        }
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        match self {
            StaticListOrVec::StaticList(a) => a.iter_mut(),
            StaticListOrVec::Vec(v) => v.iter_mut(),
        }
    }
    pub fn len(&self) -> usize {
        match self {
            StaticListOrVec::StaticList(a) => a.len(),
            StaticListOrVec::Vec(v) => v.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        match self {
            Self::StaticList(a) => a.is_empty(),
            Self::Vec(v) => v.is_empty(),
        }
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        match self {
            StaticListOrVec::StaticList(l) => l.swap(a, b),
            StaticListOrVec::Vec(v) => v.swap(a, b),
        }
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        match self {
            StaticListOrVec::StaticList(a) => a.get(index),
            StaticListOrVec::Vec(v) => v.get(index),
        }
    }
}

impl<T: Clone, const CAP: usize> Index<usize> for StaticListOrVec<T, CAP> {
    type Output = T;
    fn index(&self, index: usize) -> &Self::Output {
        match self {
            StaticListOrVec::StaticList(a) => &a[index],
            StaticListOrVec::Vec(v) => &v[index],
        }
    }
}

impl<T: Clone, const CAP: usize> IndexMut<usize> for StaticListOrVec<T, CAP> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match self {
            StaticListOrVec::StaticList(a) => &mut a[index],
            StaticListOrVec::Vec(v) => &mut v[index],
        }
    }
}

macro_rules! allocator_impl {
    ($name:ident, $unit:ident, $range:ident, $allocated:ident) => {
        /// Even though this implements Clone, you should be very careful about cloning allocators.
        #[derive(Clone)]
        pub struct $name {
            free_chunks: StaticListOrVec<$range, 256>,
        }

        impl $name {
            pub const fn new_static() -> $name {
                $name {
                    free_chunks: StaticListOrVec::new_static(),
                }
            }
            pub const fn new_vec() -> $name {
                $name {
                    free_chunks: StaticListOrVec::new_vec(),
                }
            }

            pub fn convert_to_heap_allocated(&mut self) {
                self.free_chunks.convert_to_vec();
            }

            pub fn is_region_free(&self, range: $range) -> bool {
                self.free_chunks.iter().any(|chunk| chunk.consumes(range))
            }

            /// # Safety
            /// Caller must ensure the given memory region is not in use by anything else.
            pub unsafe fn insert_free_region(&mut self, range: $range) {
                self.free_chunks.push(range)
            }

            pub fn next_free_location(&self) -> Option<&$range> {
                self.free_chunks.iter().min_by_key(|chunk| chunk.start())
            }

            pub fn max_unreserved_location(&self) -> Option<$unit> {
                self.free_chunks
                    .iter()
                    .max_by_key(|c| c.end())
                    .map(|c| c.end())
            }

            fn allocate_internal(
                &mut self,
                start: Option<$unit>,
                count: usize,
            ) -> KResult<$allocated> {
                let mut allocation: Option<$allocated> = None;
                let mut index_to_remove: Option<usize> = None;
                let mut before: Option<$range> = None;
                let mut after: Option<$range> = None;

                for (i, available_chunk) in self.free_chunks.iter().enumerate() {
                    let start = start.unwrap_or(available_chunk.start());
                    let end = start + count - 1;
                    let range_to_allocate = $range::new(start, end);
                    if available_chunk.consumes(range_to_allocate) {
                        index_to_remove = Some(i);
                        allocation =
                            Some(unsafe { $allocated::assume_allocated(range_to_allocate) });
                        if allocation
                            .as_ref()
                            .unwrap()
                            .start()
                            .index()
                            .0
                            .saturating_sub(available_chunk.start().index().0)
                            > 0
                        {
                            before = Some($range::new(available_chunk.start(), start));
                        }
                        if available_chunk
                            .end()
                            .index()
                            .0
                            .saturating_sub(allocation.as_ref().unwrap().end().index().0)
                            > 0
                        {
                            after = Some($range::new(end + 1, available_chunk.end()));
                        }
                        break;
                    }
                }

                let allocation = match allocation {
                    Some(a) => a,
                    None => return Err(kerrmsg!("couldn't find chunk for allocation")),
                };
                let index_to_remove = index_to_remove.unwrap();
                self.free_chunks.remove(index_to_remove);
                if let Some(before) = before {
                    self.free_chunks.push(before);
                }
                if let Some(after) = after {
                    self.free_chunks.push(after);
                }

                for i in 0..self.free_chunks.len() {
                    for j in 0..self.free_chunks.len() - i - 1 {
                        if self.free_chunks[j + 1].start() < self.free_chunks[j].start() {
                            self.free_chunks.swap(j, j + 1);
                        }
                    }
                }

                Ok(allocation)
            }

            pub fn merge_contiguous_chunks(&mut self) {
                let mut merge1 = None;
                let mut merge2 = None;
                for (i, chunk) in self.free_chunks.iter().enumerate() {
                    if let Some(next_chunk) = self.free_chunks.get(i + 1) {
                        if chunk.start() == next_chunk.end() + 1 {
                            merge1 = Some(*chunk);
                            merge2 = Some(*next_chunk);
                            break;
                        } else if chunk.end() + 1 == next_chunk.start() {
                            merge1 = Some(*next_chunk);
                            merge2 = Some(*chunk);
                            break;
                        }
                    }
                }
                if let Some(ref mut merge1) = merge1 {
                    let merge2 = merge2.unwrap();
                    match &mut self.free_chunks {
                        StaticListOrVec::StaticList(a) => a.retain(|chunk| {
                            chunk.start() != merge1.start() && chunk.start() != merge2.start()
                        }),
                        StaticListOrVec::Vec(v) => v.retain(|chunk| {
                            chunk.start() != merge1.start() && chunk.start() != merge2.start()
                        }),
                    }
                    if merge1.merge_with(merge2).is_err() {
                        panic!("Error merging chunks");
                    }
                    self.free_chunks.push(*merge1);

                    self.merge_contiguous_chunks();
                }
            }

            pub fn allocate(&mut self, count: usize) -> KResult<$allocated> {
                self.allocate_internal(None, count)
            }

            pub fn allocate_at(&mut self, start: $unit, count: usize) -> KResult<$allocated> {
                self.allocate_internal(Some(start), count)
            }

            pub fn allocate_range(&mut self, range: $range) -> KResult<$allocated> {
                let count = range.size_in_pages();
                self.allocate_internal(Some(range.start()), count)
            }

            pub fn free(&mut self, allocation: &mut $allocated, merge: bool) {
                if allocation.is_empty() {
                    return;
                }
                log::debug!("Freeing frame {:?}", allocation.start());
                unsafe {
                    self.insert_free_region(**allocation);
                }
                if merge {
                    self.merge_contiguous_chunks();
                }
            }
        }
    };
}

allocator_impl!(FrameAllocator, Frame, FrameRange, AllocatedFrames);
allocator_impl!(PageAllocator, Page, PageRange, AllocatedPages);
