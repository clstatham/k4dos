use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};

use spin::mutex::{SpinMutex, SpinMutexGuard};
use x86::current::rflags::{self, RFlags};
use x86_64::instructions::interrupts;

use crate::task::wait_queue::WaitQueue;
use crate::{backtrace, kerrmsg};

use super::error::KResult;

pub struct SavedInterruptStatus {
    rflags: RFlags,
}

impl SavedInterruptStatus {
    pub fn save() -> SavedInterruptStatus {
        SavedInterruptStatus {
            rflags: rflags::read(),
        }
    }
}

impl Drop for SavedInterruptStatus {
    fn drop(&mut self) {
        rflags::set(rflags::read() | (self.rflags & rflags::RFlags::FLAGS_IF));
    }
}

pub struct BlockingMutex<T: ?Sized> {
    queue: WaitQueue,
    inner: IrqMutex<T>,
}

impl<T> BlockingMutex<T> {
    pub const fn new(value: T) -> BlockingMutex<T> {
        BlockingMutex {
            queue: WaitQueue::new(),
            inner: IrqMutex::new(value),
        }
    }
}

impl<T: ?Sized> BlockingMutex<T> {
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    pub fn try_lock(&self) -> KResult<BlockingMutexGuard<'_, T>> {
        if self.inner.is_locked() {
            Err(kerrmsg!("Cannot relock BlockingMutex")) // todo: more verbose error message
        } else {
            Ok(BlockingMutexGuard {
                inner: ManuallyDrop::new(self.inner.lock()),
            })
        }
    }

    pub fn lock(&self) -> KResult<BlockingMutexGuard<'_, T>> {
        let guard = self.queue.sleep_signalable_until(None, || {
            if let Ok(guard) = self.inner.try_lock() {
                Ok(Some(BlockingMutexGuard {
                    inner: ManuallyDrop::new(guard),
                }))
            } else {
                Ok(None)
            }
        })?;
        Ok(guard)
    }

    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }

    /// # Safety
    /// See `spin::SpinMutex::force_unlock()`
    pub unsafe fn force_unlock(&self) {
        unsafe { self.inner.force_unlock() };
    }
}

unsafe impl<T: ?Sized + Send> Sync for BlockingMutex<T> {}
unsafe impl<T: ?Sized + Send> Send for BlockingMutex<T> {}

pub struct BlockingMutexGuard<'a, T: ?Sized> {
    inner: ManuallyDrop<IrqMutexGuard<'a, T>>,
}

impl<T: ?Sized> Drop for BlockingMutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.inner);
        }
    }
}

impl<T: ?Sized> Deref for BlockingMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: ?Sized> DerefMut for BlockingMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

pub struct IrqMutex<T: ?Sized> {
    inner: SpinMutex<T>,
}

impl<T> IrqMutex<T> {
    pub const fn new(value: T) -> IrqMutex<T> {
        IrqMutex {
            inner: SpinMutex::new(value),
        }
    }
}

impl<T: ?Sized> IrqMutex<T> {
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    pub fn try_lock(&self) -> KResult<IrqMutexGuard<'_, T>> {
        if self.inner.is_locked() {
            Err(kerrmsg!("Cannot relock IrqMutex")) // todo: more verbose error message
        } else {
            Ok(self.lock())
        }
    }

    pub fn lock(&self) -> IrqMutexGuard<'_, T> {
        if self.inner.is_locked() {
            serial0_println!(
                "WARNING: Tried to relock IrqMutex of {}",
                core::any::type_name::<T>()
            );
            backtrace::unwind_stack().ok();
        }

        let saved_intr_status = SavedInterruptStatus::save();
        interrupts::disable();

        let guard = self.inner.lock();

        IrqMutexGuard {
            inner: ManuallyDrop::new(guard),
            saved_intr_status: ManuallyDrop::new(saved_intr_status),
        }
    }

    pub fn is_locked(&self) -> bool {
        self.inner.is_locked()
    }

    /// # Safety
    /// See `spin::SpinMutex::force_unlock()`
    pub unsafe fn force_unlock(&self) {
        unsafe { self.inner.force_unlock() };
    }
}

unsafe impl<T: ?Sized + Send> Sync for IrqMutex<T> {}
unsafe impl<T: ?Sized + Send> Send for IrqMutex<T> {}

pub struct IrqMutexGuard<'a, T: ?Sized> {
    inner: ManuallyDrop<SpinMutexGuard<'a, T>>,
    saved_intr_status: ManuallyDrop<SavedInterruptStatus>,
}

impl<T: ?Sized> Drop for IrqMutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.inner);
        }

        unsafe {
            ManuallyDrop::drop(&mut self.saved_intr_status);
        }
    }
}

impl<T: ?Sized> Deref for IrqMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<T: ?Sized> DerefMut for IrqMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}
