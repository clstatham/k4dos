use core::mem::size_of;

use super::align_down;

pub struct Stack<'a> {
    ptr: &'a mut usize,
}

impl<'a> Stack<'a> {
    pub fn new(ptr: &'a mut usize) -> Self {
        Self { ptr }
    }

    pub fn skip_by(&mut self, by: usize) {
        *self.ptr -= by;
    }

    pub unsafe fn offset<T: Sized>(&mut self) -> &mut T {
        self.skip_by(size_of::<T>());
        &mut *(*self.ptr as *mut T)
    }

    pub fn top(&self) -> usize {
        *self.ptr
    }

    pub unsafe fn push_bytes(&mut self, bytes: &[u8]) {
        self.skip_by(bytes.len());

        (*self.ptr as *mut u8).copy_from(bytes.as_ptr(), bytes.len());
    }

    pub unsafe fn push<T: Sized>(&mut self, value: T) {
        self.skip_by(size_of::<T>());
        (*self.ptr as *mut T).write(value);
    }

    pub fn pop_by(&mut self, by: usize) {
        *self.ptr += by;
    }

    pub unsafe fn pop_bytes(&mut self, len: usize) -> &[u8] {
        let x = core::slice::from_raw_parts(*self.ptr as *const u8, len);
        self.pop_by(len);
        x
    }

    pub unsafe fn pop<'b, T: Sized>(&mut self) -> &'b mut T {
        let x = &mut *(*self.ptr as *mut T);
        self.pop_by(size_of::<T>());
        x
    }

    pub fn align_down(&mut self, align: usize) {
        *self.ptr = align_down(*self.ptr, align)
    }
}
