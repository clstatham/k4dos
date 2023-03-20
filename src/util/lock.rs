use core::arch::asm;
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};

use spin::mutex::{SpinMutex, SpinMutexGuard};
use x86::current::rflags::{self, RFlags};

// use crate::interrupts::SavedInterruptStatus;
use crate::{backtrace, kerrmsg, terminal_println};

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

pub struct SpinLock<T: ?Sized> {
    inner: SpinMutex<T>,
}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> SpinLock<T> {
        SpinLock {
            inner: SpinMutex::new(value),
        }
    }
}

impl<T: ?Sized> SpinLock<T> {
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }

    pub fn try_lock(&self) -> KResult<SpinLockGuard<'_, T>> {
        if self.inner.is_locked() {
            Err(kerrmsg!("Cannot relock SpinLock")) // todo: more verbose error message
        } else {
            Ok(self.lock())
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        if self.inner.is_locked() {
            serial0_println!(
                "WARNING: Tried to relock SpinLock of {}",
                core::any::type_name::<T>()
            );
            // backtrace::backtrace();
            backtrace::unwind_stack().unwrap();
        }

        let saved_intr_status = SavedInterruptStatus::save();
        unsafe {
            asm!("cli");
        }

        let guard = self.inner.lock();

        SpinLockGuard {
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
        self.inner.force_unlock();
    }
}

unsafe impl<T: ?Sized + Send> Sync for SpinLock<T> {}
unsafe impl<T: ?Sized + Send> Send for SpinLock<T> {}

pub struct SpinLockGuard<'a, T: ?Sized> {
    inner: ManuallyDrop<SpinMutexGuard<'a, T>>,
    saved_intr_status: ManuallyDrop<SavedInterruptStatus>,
}

impl<'a, T: ?Sized> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.inner);
        }

        unsafe {
            ManuallyDrop::drop(&mut self.saved_intr_status);
        }
    }
}

impl<'a, T: ?Sized> Deref for SpinLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.inner
    }
}

impl<'a, T: ?Sized> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}
