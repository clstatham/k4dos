use core::borrow::BorrowMut;

use alloc::{borrow::ToOwned, string::String, sync::Arc, vec::Vec};
use atomic_refcell::AtomicRefCell;
use bitflags::bitflags;
use crossbeam_utils::atomic::AtomicCell;

use crate::{
    errno,
    util::{ctypes::c_int, errno::Errno, error::KResult}, userland::buffer::{UserBufferMut, UserBuffer},
};

use super::{
    path::{PathBuf, PathComponent}, DirEntry, DirRef, FileRef, INode, PollStatus
};

const FD_MAX: c_int = 1024;

bitflags! {
    pub struct OpenFlags: i32 {
        const O_RDONLY = 0;
        const O_WRONLY = 1;
        const O_RDWR = 2;
        const O_CREAT = 100;
        const O_EXCL = 200;
        const O_NOCTTY = 400;
        const O_TRUNC = 1000;
        const O_APPEND = 2000;
        const O_NONBLOCK = 4000;
        const O_DIRECTORY = 200000;
        const O_CLOEXEC  = 2000000;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpenOptions {
    pub nonblock: bool,
    pub close_on_exec: bool,
}

impl OpenOptions {
    pub fn new(nonblock: bool, close_on_exec: bool) -> OpenOptions {
        OpenOptions {
            nonblock,
            close_on_exec,
        }
    }

    pub fn empty() -> OpenOptions {
        OpenOptions {
            nonblock: false,
            close_on_exec: false,
        }
    }

    pub fn readwrite() -> OpenOptions {
        OpenOptions {
            nonblock: false,
            close_on_exec: false,
        }
    }
}

impl From<OpenFlags> for OpenOptions {
    fn from(value: OpenFlags) -> Self {
        OpenOptions {
            nonblock: value.contains(OpenFlags::O_NONBLOCK),
            close_on_exec: value.contains(OpenFlags::O_CLOEXEC),
        }
    }
}

pub type FileDesc = c_int;


pub struct OpenedFile {
    path: Arc<PathComponent>,
    pos: AtomicCell<usize>,
    options: AtomicRefCell<OpenOptions>,
}

impl OpenedFile {
    pub fn new(path: Arc<PathComponent>, options: OpenOptions, pos: usize) -> OpenedFile {
        OpenedFile {
            path,
            pos: AtomicCell::new(pos),
            options: AtomicRefCell::new(options),
        }
    }

    pub fn as_file(&self) -> KResult<&FileRef> {
        self.path.inode.as_file()
    }

    pub fn as_dir(&self) -> KResult<&DirRef> {
        self.path.inode.as_dir()
    }

    pub fn pos(&self) -> usize {
        self.pos.load()
    }

    pub fn options(&self) -> OpenOptions {
        *self.options.borrow()
    }

    pub fn path(&self) -> &Arc<PathComponent> {
        &self.path
    }

    pub fn inode(&self) -> &INode {
        &self.path.inode
    }

    pub fn read(&self, buf: UserBufferMut) -> KResult<usize> {
        let options = self.options();
        let pos = self.pos();
        let read_len = self.as_file()?.read(pos, buf, &options)?;
        self.pos.fetch_add(read_len);
        Ok(read_len)
    }

    pub fn write(&self, buf: UserBuffer) -> KResult<usize> {
        let options = self.options();
        let pos = self.pos();
        let written_len = self.as_file()?.write(pos, buf, &options)?;
        self.pos.fetch_add(written_len);
        Ok(written_len)
    }

    pub fn set_close_on_exec(&self, close_on_exec: bool) {
        self.options().borrow_mut().close_on_exec = close_on_exec;
    }

    pub fn set_flags(&self, flags: OpenFlags) -> KResult<()> {
        if flags.contains(OpenFlags::O_NONBLOCK) {
            self.options.borrow_mut().nonblock = true;
        }

        Ok(())
    }


    pub fn poll(&self) -> KResult<PollStatus> {
        self.as_file()?.poll()
    }
}

#[derive(Clone)]
struct LocalOpenedFile {
    opened_file: Arc<OpenedFile>,
    close_on_exec: bool,
}

#[derive(Clone)]
pub struct OpenedFileTable {
    files: Vec<Option<LocalOpenedFile>>,
    prev_fd: i32,
}

impl Default for OpenedFileTable {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenedFileTable {
    pub fn new() -> OpenedFileTable {
        OpenedFileTable {
            files: Vec::new(),
            prev_fd: 1,
        }
    }

    pub fn get(&self, fd: FileDesc) -> KResult<&Arc<OpenedFile>> {
        match self.files.get(fd as usize) {
            Some(Some(LocalOpenedFile { opened_file, .. })) => Ok(opened_file),
            _ => Err(errno!(Errno::EBADF)),
        }
    }

    pub fn open(&mut self, path: Arc<PathComponent>, options: OpenOptions) -> KResult<FileDesc> {
        self.alloc_fd(None).and_then(|fd| {
            self.open_with_fd(
                fd,
                Arc::new(OpenedFile {
                    path,
                    options: AtomicRefCell::new(options),
                    pos: AtomicCell::new(0),
                }),
                options,
            )
            .map(|_| fd)
        })
    }

    pub fn open_with_fd(
        &mut self,
        fd: FileDesc,
        mut opened_file: Arc<OpenedFile>,
        options: OpenOptions,
    ) -> KResult<()> {
        if let INode::File(file) = &opened_file.path.inode {
            if let Some(new_inode) = file.open(&options)? {
                opened_file = Arc::new(OpenedFile {
                    pos: AtomicCell::new(0),
                    options: AtomicRefCell::new(options),
                    path: Arc::new(PathComponent {
                        parent_dir: opened_file.path.parent_dir.clone(),
                        name: opened_file.path.name.clone(),
                        inode: INode::File(new_inode),
                    }),
                });
            }
        }

        match self.files.get_mut(fd as usize) {
            Some(Some(_)) => return Err(errno!(Errno::EBADF)),
            Some(entry @ None) => {
                *entry = Some(LocalOpenedFile {
                    opened_file,
                    close_on_exec: options.close_on_exec,
                });
            }
            None if fd >= FD_MAX => return Err(errno!(Errno::EBADF)),
            None => {
                self.files.resize(fd as usize + 1, None);
                self.files[fd as usize] = Some(LocalOpenedFile {
                    opened_file,
                    close_on_exec: options.close_on_exec,
                })
            }
        }

        Ok(())
    }

    fn alloc_fd(&mut self, gte: Option<i32>) -> KResult<FileDesc> {
        let (mut i, gte) = match gte {
            Some(gte) => (gte, gte),
            None => ((self.prev_fd + 1) % FD_MAX, 0),
        };

        while i != self.prev_fd && i >= gte {
            if matches!(self.files.get(i as usize), Some(None) | None) {
                return Ok(i);
            }

            i = (i + 1) % FD_MAX;
        }

        Err(errno!(Errno::ENFILE))
    }

    pub fn close_all(&mut self) {
        self.files.clear()
    }

    pub fn close_cloexec_files(&mut self) {
        for opened_file in &mut self.files {
            if matches!(
                opened_file,
                Some(LocalOpenedFile {
                    close_on_exec: true,
                    ..
                })
            ) {
                *opened_file = None;
            }
        }
    }

    pub fn close(&mut self, fd: FileDesc) -> KResult<()> {
        match self.files.get_mut(fd as usize) {
            Some(opened_file) => *opened_file = None,
            _ => return Err(errno!(Errno::EBADF)),
        }
        Ok(())
    }

    pub fn dup(
        &mut self,
        fd: FileDesc,
        gte: Option<i32>,
        options: OpenOptions,
    ) -> KResult<FileDesc> {
        let file = match self.files.get(fd as usize) {
            Some(Some(file)) => file.opened_file.clone(),
            _ => return Err(errno!(Errno::EBADF)),
        };

        self.alloc_fd(gte)
            .and_then(|fd| self.open_with_fd(fd, file, options).map(|_| fd))
    }
}
