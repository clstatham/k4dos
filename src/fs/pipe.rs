use alloc::{format, sync::Arc, vec::Vec};

use crate::{
    errno,
    task::wait_queue::WaitQueue,
    userland::buffer::{UserBuffer, UserBufferMut, UserBufferReader, UserBufferWriter},
    util::{errno::Errno, ringbuffer::RingBuffer, IrqMutex, KResult},
};

use super::{opened_file::FileDesc, path::Path, File, FsNode};

pub static PIPE_FS: PipeFs = PipeFs::new();

pub struct Pipe {
    wait_queue: WaitQueue,
    ringbuffer: Arc<IrqMutex<RingBuffer<u8, 65536>>>,
    read_fd: FileDesc,
    write_fd: FileDesc,
}

impl Pipe {
    pub fn new(read_fd: FileDesc, write_fd: FileDesc) -> Self {
        Self {
            wait_queue: WaitQueue::new(),
            ringbuffer: Arc::new(IrqMutex::new(RingBuffer::new())),
            read_fd,
            write_fd,
        }
    }

    pub fn read_pipe(&self, buf: UserBufferMut<'_>) -> KResult<usize> {
        let mut writer = UserBufferWriter::from(buf);
        let mut ringbuffer = self.wait_queue.sleep_signalable_until(None, || {
            let ringbuffer = self.ringbuffer.try_lock();
            if let Ok(ringbuffer) = ringbuffer {
                if ringbuffer.is_readable() {
                    Ok(Some(ringbuffer))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        })?;
        while let Some(byte) = ringbuffer.pop() {
            writer.write(byte)?;
        }
        Ok(writer.written_len())
    }

    pub fn write_pipe(&self, buf: UserBuffer<'_>) -> KResult<usize> {
        let mut reader = UserBufferReader::from(buf);
        let mut ringbuffer = self.wait_queue.sleep_signalable_until(None, || {
            let ringbuffer = self.ringbuffer.try_lock();
            if let Ok(ringbuffer) = ringbuffer {
                if ringbuffer.is_writable() {
                    Ok(Some(ringbuffer))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        })?;
        while let Ok(byte) = reader.read::<u8>() {
            ringbuffer.push(byte).ok();
        }
        Ok(reader.read_len())
    }

    pub fn read_fd(&self) -> FileDesc {
        self.read_fd
    }

    pub fn write_fd(&self) -> FileDesc {
        self.write_fd
    }
}

impl FsNode for Pipe {
    fn get_name(&self) -> alloc::string::String {
        format!("pipe_{}_{}", self.write_fd, self.read_fd)
    }
}

impl File for Pipe {
    fn read(
        &self,
        _offset: usize,
        buf: UserBufferMut,
        _options: &super::opened_file::OpenOptions,
        // len: usize,
    ) -> KResult<usize> {
        self.read_pipe(buf)
    }

    fn write(
        &self,
        _offset: usize,
        buf: UserBuffer<'_>,
        _options: &super::opened_file::OpenOptions,
    ) -> KResult<usize> {
        self.write_pipe(buf)
    }
}

pub struct PipeFs {
    pipes: IrqMutex<Vec<Arc<Pipe>>>,
}

impl PipeFs {
    pub const fn new() -> Self {
        Self {
            pipes: IrqMutex::new(Vec::new()),
        }
    }

    pub fn insert(&self, pipe: Arc<Pipe>) {
        self.pipes.lock().push(pipe);
    }

    pub fn lookup(&self, path: &Path) -> KResult<Arc<Pipe>> {
        self.pipes
            .lock()
            .iter()
            .find(|pipe| pipe.get_name() == path.pipe_name().unwrap())
            .cloned()
            .ok_or(errno!(Errno::ENOENT, "pipe does not exist"))
    }
}
