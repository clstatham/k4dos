
use core::{mem::size_of, ops::Add};

use crate::{mem::addr::VirtAddr, util::{ctypes::{c_nfds, c_int, c_short}, KResult}, fs::{POLL_WAIT_QUEUE, opened_file::FileDesc, PollStatus}, userland::buffer::{UserBufferReader, UserBuffer}, bitflags_from_user, task::current_task};

use super::SyscallHandler;

impl<'a> SyscallHandler<'a> {
    pub fn sys_poll(&mut self, fds: VirtAddr, nfds: c_nfds, timeout: c_int) -> KResult<isize> {
        if timeout > 0 {
            log::debug!("Ignoring timeout of {} ms.", timeout);
        }

        POLL_WAIT_QUEUE.sleep_signalable_until(|| {
            // todo: check timeout

            let mut ready_fds = 0;
            let fds_len = (nfds as usize) * (size_of::<FileDesc>() + 2 * size_of::<c_short>());
            let mut reader = UserBufferReader::from(UserBuffer::from_vaddr(fds, fds_len));
            for _ in 0..nfds {
                let fd = reader.read::<FileDesc>()?;
                log::debug!("fd: {:?}", fd);
                let events = bitflags_from_user!(PollStatus, reader.read::<c_short>()?);
                log::debug!("events: {:?}", events);
                let revents = if fd < 0 || events.is_empty() {
                    0
                } else {
                    let status = current_task().opened_files.lock().get(fd)?.poll()?;
                    log::debug!("status: {:?}", status);
                    let revents = events & status;
                    if !revents.is_empty() {
                        ready_fds += 1;
                    }
                    log::debug!("revents: {:?}", revents);
                    revents.bits()
                };
                

                fds.add(reader.read_len()).write::<c_short>(revents)?;

                reader.skip(size_of::<c_short>())?;
            }

            if ready_fds > 0 {
                Ok(Some(ready_fds))
            } else {
                Ok(None)
            }
        })
    }
}
