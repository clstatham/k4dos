use core::ops::{Index, IndexMut};

use alloc::vec::Vec;
use arrayvec::ArrayVec;
use buddy_system_allocator::LockedHeap;
use limine::memory_map::{Entry, EntryType};
use spin::Once;

use super::addr::{PhysAddr, VirtAddr};
use super::consts::{MAX_LOW_VADDR, MIN_HIGH_VADDR, PAGE_SIZE};
use super::paging::units::{
    Allocated, AllocatedFrames, AllocatedPages, Frame, FrameRange, MemoryRange, MemoryUnit, Page,
    PageIndex, PageRange,
};

use crate::kerror;
use crate::util::{align_down, IrqMutex, KResult};

pub static KERNEL_FRAME_ALLOCATOR: Once<IrqMutex<FrameAllocator>> = Once::new();
pub static KERNEL_PAGE_ALLOCATOR: Once<IrqMutex<PageAllocator>> = Once::new();

#[global_allocator]
pub static GLOBAL_ALLOC: LockedHeap<32> = LockedHeap::new();

pub fn alloc_kernel_frames(count: usize) -> KResult<AllocatedFrames> {
    KERNEL_FRAME_ALLOCATOR
        .get()
        .ok_or(kerror!("KERNEL_FRAME_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate(count)
}

pub fn alloc_kernel_frames_at(start: Frame, count: usize) -> KResult<AllocatedFrames> {
    KERNEL_FRAME_ALLOCATOR
        .get()
        .ok_or(kerror!("KERNEL_FRAME_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate_at(start, count)
}

pub fn alloc_kernel_pages(count: usize) -> KResult<AllocatedPages> {
    KERNEL_PAGE_ALLOCATOR
        .get()
        .ok_or(kerror!("KERNEL_PAGE_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate(count)
}

pub fn alloc_kernel_pages_at(start: Page, count: usize) -> KResult<AllocatedPages> {
    KERNEL_PAGE_ALLOCATOR
        .get()
        .ok_or(kerror!("KERNEL_PAGE_ALLOCATOR not initialized"))?
        .try_lock()?
        .allocate_at(start, count)
}

pub fn free_kernel_frames(frames: &mut AllocatedFrames, merge: bool) -> KResult<()> {
    KERNEL_FRAME_ALLOCATOR
        .get()
        .ok_or(kerror!("KERNEL_FRAME_ALLOCATOR not initialized"))?
        .try_lock()?
        .free(frames, merge);
    Ok(())
}

pub fn free_kernel_pages(pages: &mut AllocatedPages, merge: bool) -> KResult<()> {
    KERNEL_PAGE_ALLOCATOR
        .get()
        .ok_or(kerror!("KERNEL_PAGE_ALLOCATOR not initialized"))?
        .try_lock()?
        .free(pages, merge);
    Ok(())
}

pub fn init(memmap: &[&Entry]) -> KResult<()> {
    let mut frame_alloc = FrameAllocator::new_static();
    for entry in memmap
        .iter()
        .filter(|entry| entry.entry_type == EntryType::USABLE)
    {
        if (entry.length as usize) < PAGE_SIZE {
            continue;
        }
        let start = entry.base as usize;
        let end = start + entry.length as usize;
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
        Page::containing_address(MAX_LOW_VADDR - 1),
    );
    unsafe { page_alloc.insert_free_region(pages) };
    let pages = PageRange::new(
        Page::containing_address(MIN_HIGH_VADDR),
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

#[derive(Clone)]
pub struct Allocator<T: MemoryUnit> {
    free_regions: StaticListOrVec<MemoryRange<T>, 32>,
}

impl<T: MemoryUnit> Allocator<T> {
    pub fn new_static() -> Self {
        Self {
            free_regions: StaticListOrVec::new_static(),
        }
    }

    pub fn new_vec() -> Self {
        Self {
            free_regions: StaticListOrVec::new_vec(),
        }
    }

    pub fn convert_to_heap_allocated(&mut self) {
        self.free_regions.convert_to_vec();
    }

    pub fn free_regions(&self) -> impl Iterator<Item = &MemoryRange<T>> {
        self.free_regions.iter()
    }

    pub unsafe fn insert_free_region(&mut self, range: MemoryRange<T>) {
        self.free_regions.push(range);
    }

    pub fn allocate(&mut self, count: usize) -> KResult<Allocated<T>> {
        if count == 0 {
            return Err(kerror!("Cannot allocate 0 units"));
        }

        let mut best_fit = None;
        for region in self.free_regions.iter() {
            if region.size_in_pages() >= count {
                best_fit = Some(*region);
                break;
            }
        }
        let best_fit = best_fit.ok_or(kerror!("Out of memory"))?;
        let new_region = MemoryRange::new(best_fit.start, best_fit.start + count);
        if best_fit.size_in_pages() == count {
            self.free_regions.remove(0);
        } else {
            self.free_regions[0] = MemoryRange::new(best_fit.start + count, best_fit.end);
        }
        Ok(unsafe { Allocated::assume_allocated(new_region) })
    }

    pub fn allocate_at(&mut self, start: T, count: usize) -> KResult<Allocated<T>> {
        if count == 0 {
            return Err(kerror!("Cannot allocate 0 units"));
        }

        let mut best_fit = None;
        for region in self.free_regions.iter() {
            if region.start() <= start && region.end() >= start + count {
                best_fit = Some(*region);
                break;
            }
        }
        let best_fit = best_fit.ok_or(kerror!("Out of memory"))?;
        let new_region = MemoryRange::new(start, start + count);
        if best_fit.start() == start {
            if best_fit.size_in_pages() == count {
                self.free_regions.remove(0);
            } else {
                self.free_regions[0] = MemoryRange::new(start + count, best_fit.end);
            }
        } else if best_fit.end() == start + count {
            self.free_regions[0] = MemoryRange::new(best_fit.start, start);
        } else {
            self.free_regions[0] = MemoryRange::new(best_fit.start, start);
            self.free_regions
                .push(MemoryRange::new(start + count, best_fit.end));
        }
        Ok(unsafe { Allocated::assume_allocated(new_region) })
    }

    pub fn free(&mut self, allocated: &mut Allocated<T>, merge: bool) {
        let mut new_region = allocated.range;
        if merge {
            for region in self.free_regions.iter_mut() {
                if region.start() == new_region.end() {
                    new_region = MemoryRange::new(new_region.start(), region.end());
                    *region = new_region;
                    return;
                } else if region.end() == new_region.start() {
                    new_region = MemoryRange::new(region.start(), new_region.end());
                    *region = new_region;
                    return;
                }
            }
        }
        self.free_regions.push(new_region);
    }
}

pub type FrameAllocator = Allocator<Frame>;
pub type PageAllocator = Allocator<Page>;
