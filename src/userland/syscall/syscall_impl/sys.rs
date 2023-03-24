use crate::{
    mem::addr::VirtAddr,
    userland::{buffer::UserBufferWriter, syscall::SyscallHandler},
    util::KResult,
};

const UTS_FIELD_LEN: usize = 65;

impl<'a> SyscallHandler<'a> {
    pub fn sys_uname(&mut self, buf: VirtAddr) -> KResult<isize> {
        let mut writer = UserBufferWriter::from_vaddr(buf, UTS_FIELD_LEN * 6);
        writer.write_bytes_or_zeroes(b"Linux", UTS_FIELD_LEN)?;
        writer.write_bytes_or_zeroes(b"", UTS_FIELD_LEN)?;
        writer.write_bytes_or_zeroes(b"4.0.0", UTS_FIELD_LEN)?;
        writer.write_bytes_or_zeroes(b"K4DOS", UTS_FIELD_LEN)?;
        writer.write_bytes_or_zeroes(b"", UTS_FIELD_LEN)?;
        writer.write_bytes_or_zeroes(b"", UTS_FIELD_LEN)?;
        Ok(0)
    }
}
