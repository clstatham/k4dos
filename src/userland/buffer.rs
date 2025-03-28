use core::{mem::size_of, ops::Add};

use alloc::string::{String, ToString};

use crate::{
    kbail, kerror,
    mem::{addr::VirtAddr, addr_space::TmpAddrSpaceGuard},
    task::current_task,
    util::{align_up, error::KResult},
};

#[allow(dead_code)]
enum Inner<'a> {
    Slice(&'a [u8]),
    User { base: VirtAddr, len: usize },
}

pub struct UserBuffer<'a> {
    inner: Inner<'a>,
}

impl<'a> UserBuffer<'a> {
    pub fn from_vaddr(vaddr: VirtAddr, len: usize) -> UserBuffer<'static> {
        UserBuffer {
            inner: Inner::User { base: vaddr, len },
        }
    }

    pub fn from_slice(slc: &'a [u8]) -> UserBuffer<'a> {
        UserBuffer {
            inner: Inner::Slice(slc),
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match &self.inner {
            Inner::Slice(slice) => slice.len(),
            Inner::User { len, .. } => *len,
        }
    }
}

#[allow(dead_code)]
enum InnerMut<'a> {
    Slice(&'a mut [u8]),
    User { base: VirtAddr, len: usize },
}

pub struct UserBufferMut<'a> {
    inner: InnerMut<'a>,
}

impl<'a> UserBufferMut<'a> {
    pub fn from_slice(slice: &'a mut [u8]) -> UserBufferMut<'a> {
        UserBufferMut {
            inner: InnerMut::Slice(slice),
        }
    }

    pub fn from_vaddr(vaddr: VirtAddr, len: usize) -> UserBufferMut<'static> {
        UserBufferMut {
            inner: InnerMut::User { base: vaddr, len },
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        match &self.inner {
            InnerMut::Slice(slice) => slice.len(),
            InnerMut::User { len, .. } => *len,
        }
    }
}

pub unsafe fn user_strncpy_rust(dst: *mut u8, src: *const u8, max_len: usize) -> usize {
    let mut read_len = 0usize;
    loop {
        let byte = unsafe { src.add(read_len).read_volatile() };
        if byte == b'\0' || read_len > max_len {
            break;
        }
        unsafe { dst.add(read_len).write_volatile(byte) };
        read_len += 1;
    }
    read_len
}

pub struct CStr {
    string: String,
}

impl CStr {
    pub fn new(vaddr: VirtAddr, max_len: usize, is_user: bool) -> KResult<CStr> {
        vaddr.align_ok::<u8>()?;
        if is_user {
            (vaddr + max_len).user_ok()?;
        }
        let mut tmp = alloc::vec![0; max_len];
        let guard = current_task().arch_mut().address_space.temporarily_switch();
        let read_len = unsafe { user_strncpy_rust(tmp.as_mut_ptr(), vaddr.as_raw_ptr(), max_len) };
        drop(guard);
        let string = core::str::from_utf8(&tmp[..read_len])
            .map_err(|_| kerror!(EINVAL, "UserCStr: UTF-8 parsing error"))?
            .to_string();
        Ok(CStr { string })
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.string.as_bytes()
    }

    pub fn as_str(&self) -> &str {
        &self.string
    }
}

pub struct UserBufferReader<'a> {
    buf: UserBuffer<'a>,
    pos: usize,
    #[allow(dead_code)]
    guard: TmpAddrSpaceGuard,
}

impl<'a> UserBufferReader<'a> {
    pub fn from_vaddr(buf: VirtAddr, len: usize) -> UserBufferReader<'a> {
        let guard = current_task().arch_mut().address_space.temporarily_switch();
        UserBufferReader {
            buf: UserBuffer::from_vaddr(buf, len),
            pos: 0,
            guard,
        }
    }

    pub fn from_buf(buf: UserBuffer<'a>) -> UserBufferReader<'a> {
        let guard = current_task().arch_mut().address_space.temporarily_switch();
        UserBufferReader { buf, pos: 0, guard }
    }

    pub fn read_len(&self) -> usize {
        self.pos
    }

    pub fn skip(&mut self, len: usize) -> KResult<()> {
        self.check_remaining_len(len)?;
        self.pos += len;
        Ok(())
    }

    pub fn read_bytes(&mut self, dst: &mut [u8]) -> KResult<usize> {
        let read_len = core::cmp::min(dst.len(), self.remaining_len());
        if read_len == 0 {
            return Ok(0);
        }

        match &self.buf.inner {
            Inner::Slice(src) => {
                dst[..read_len].copy_from_slice(&src[self.pos..(self.pos + read_len)])
            }
            Inner::User { base, .. } => unsafe {
                base.add(self.pos).read_bytes(&mut dst[..read_len])?;
            },
        }

        self.pos += read_len;

        Ok(read_len)
    }

    pub fn read<T: Copy + Sized>(&mut self) -> KResult<T> {
        self.check_remaining_len(size_of::<T>())?;

        let val = match &self.buf.inner {
            Inner::Slice(src) => {
                // this could cause a page fault if the inner slice of the buffer isn't mapped to the current page table!
                unsafe { *(src.as_ptr().add(self.pos) as *const T) }
            }
            Inner::User { base, .. } => unsafe { base.add(self.pos).read()? },
        };

        self.pos += size_of::<T>();

        Ok(val)
    }

    fn check_remaining_len(&self, len: usize) -> KResult<()> {
        if len <= self.remaining_len() {
            Ok(())
        } else {
            kbail!(EINVAL, "check_remaining_len(): len out of bounds");
        }
    }

    pub fn remaining_len(&self) -> usize {
        self.buf.len() - self.pos
    }
}

pub struct UserBufferWriter<'a> {
    buf: UserBufferMut<'a>,
    pos: usize,
    #[allow(dead_code)]
    guard: TmpAddrSpaceGuard,
}

impl<'a> UserBufferWriter<'a> {
    pub fn from_vaddr(buf: VirtAddr, len: usize) -> UserBufferWriter<'a> {
        let guard = current_task().arch_mut().address_space.temporarily_switch();
        UserBufferWriter {
            buf: UserBufferMut::from_vaddr(buf, len),
            pos: 0,
            guard,
        }
    }

    pub fn from_buf(buf: UserBufferMut<'a>) -> UserBufferWriter<'a> {
        let guard = current_task().arch_mut().address_space.temporarily_switch();
        UserBufferWriter { buf, pos: 0, guard }
    }

    pub fn written_len(&self) -> usize {
        self.pos
    }

    pub fn write<T: Copy + Sized>(&mut self, value: T) -> KResult<usize> {
        let bytes =
            unsafe { core::slice::from_raw_parts(&value as *const T as *const u8, size_of::<T>()) };
        self.write_bytes(bytes)
    }

    pub fn write_bytes(&mut self, src: &[u8]) -> KResult<usize> {
        let copy_len = core::cmp::min(self.remaining_len(), src.len());
        if copy_len == 0 {
            return Ok(0);
        }
        self.check_remaining_len(copy_len)?;

        match &mut self.buf.inner {
            InnerMut::Slice(dst) => {
                dst[self.pos..(self.pos + copy_len)].copy_from_slice(&src[..copy_len]);
            }
            InnerMut::User { base, .. } => {
                unsafe { base.add(self.pos).write_bytes(&src[..copy_len]) }?;
            }
        }

        self.pos += copy_len;
        Ok(copy_len)
    }

    pub fn skip_until_alignment(&mut self, alignment: usize) -> KResult<()> {
        let new_pos = align_up(self.pos, alignment);
        self.check_remaining_len(new_pos - self.pos)?;
        self.pos = new_pos;
        Ok(())
    }

    pub fn fill(&mut self, value: u8, len: usize) -> KResult<()> {
        self.check_remaining_len(len)?;

        match &mut self.buf.inner {
            InnerMut::Slice(dst) => {
                dst[self.pos..(self.pos + len)].fill(value);
            }
            InnerMut::User { base, .. } => {
                unsafe { base.add(self.pos).fill(value, len) }?;
            }
        }

        self.pos += len;
        Ok(())
    }

    pub fn write_bytes_or_zeros(&mut self, buf: &[u8], max_len: usize) -> KResult<()> {
        let zero_start = core::cmp::min(buf.len(), max_len);
        self.check_remaining_len(zero_start)?;
        self.write_bytes(&buf[..zero_start])?;
        self.fill(0, max_len - zero_start)?;
        Ok(())
    }

    fn check_remaining_len(&self, len: usize) -> KResult<()> {
        if len <= self.remaining_len() {
            Ok(())
        } else {
            kbail!(EINVAL, "check_remaining_len(): len out of bounds");
        }
    }

    pub fn remaining_len(&self) -> usize {
        self.buf.len() - self.pos
    }
}
