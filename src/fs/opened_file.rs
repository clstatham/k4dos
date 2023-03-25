use core::borrow::BorrowMut;

use alloc::{sync::Arc, vec::Vec};
use atomic_refcell::AtomicRefCell;
use bitflags::bitflags;
use crossbeam_utils::atomic::AtomicCell;

use crate::{
    errno,
    userland::buffer::{UserBuffer, UserBufferMut},
    util::{ctypes::c_int, errno::Errno, error::KResult},
};

use super::{
    path::PathComponent,
    pipe::{Pipe, PIPE_FS},
    DirEntry, DirRef, FileRef, FsNode, INode, PollStatus,
};

const FD_MAX: c_int = 1024;

bitflags! {
    #[derive(Clone, Copy)]
    pub struct OpenFlags: i32 {
        const O_RDONLY    = 0o0;
        const O_WRONLY    = 0o1;
        const O_RDWR      = 0o2;
        const O_CREAT     = 0o0100;
        const O_EXCL      = 0o0200;
        const O_NOCTTY    = 0o0400;
        const O_TRUNC     = 0o01000;
        const O_APPEND    = 0o02000;
        const O_NONBLOCK  = 0o04000;
        const O_DSYNC     = 0o010000;
        const O_SYNC      = 0o04010000;
        const O_RSYNC     = 0o04010000;
        const O_DIRECTORY = 0o0200000;
        const O_NOFOLLOW  = 0o0400000;
        const O_CLOEXEC   = 0o02000000;
        const O_ASYNC     = 0o020000;
        const O_DIRECT    = 0o040000;
        const O_LARGEFILE = 0o0100000;
        const O_NOATIME   = 0o01000000;
        const O_PATH      = 0o010000000;
        const O_TMPFILE   = 0o020200000;
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

    pub fn get_flags(&self) -> OpenFlags {
        if self.options.borrow_mut().close_on_exec {
            OpenFlags::O_CLOEXEC
        } else {
            OpenFlags::empty()
        }
    }

    pub fn poll(&self) -> KResult<PollStatus> {
        self.as_file()?.poll()
    }

    pub fn ioctl(&self, cmd: usize, arg: usize) -> KResult<isize> {
        self.as_file()?.ioctl(cmd, arg)
    }

    pub fn readdir(&self) -> KResult<Option<DirEntry>> {
        let pos = self.pos();

        let entry = self.as_dir()?.readdir(pos)?;
        self.pos.fetch_add(1);
        Ok(entry)
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
            _ => Err(errno!(Errno::EBADF, "get(): file not opened")),
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
            Some(Some(_)) => {
                return Err(errno!(Errno::EBADF, "open_with_fd(): file already opened"))
            }
            Some(entry @ None) => {
                *entry = Some(LocalOpenedFile {
                    opened_file,
                    close_on_exec: options.close_on_exec,
                });
            }
            None if fd >= FD_MAX => {
                return Err(errno!(
                    Errno::EBADF,
                    "open_with_fd(): maximum file descriptor reached"
                ))
            }
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

        Err(errno!(
            Errno::ENFILE,
            "alloc_fd(): cannot alloc file descriptor"
        ))
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
            _ => return Err(errno!(Errno::EBADF, "close(): file not opened")),
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
            _ => return Err(errno!(Errno::EBADF, "dup(): file not opened")),
        };

        self.alloc_fd(gte)
            .and_then(|fd| self.open_with_fd(fd, file, options).map(|_| fd))
    }

    pub fn open_pipe(&mut self, options: OpenOptions) -> KResult<Arc<Pipe>> {
        let write_fd = self.alloc_fd(None)?;
        let read_fd = self.alloc_fd(Some(write_fd + 1))?;
        let pipe = Arc::new(Pipe::new(read_fd, write_fd));
        PIPE_FS.insert(pipe.clone());

        self.files.resize(read_fd as usize + 1, None);
        self.open_with_fd(
            write_fd,
            OpenedFile::new(
                Arc::new(PathComponent {
                    parent_dir: None,
                    name: pipe.get_name(),
                    inode: INode::Pipe(pipe.clone()),
                }),
                options,
                0,
            )
            .into(),
            options,
        )?;
        self.open_with_fd(
            read_fd,
            OpenedFile::new(
                Arc::new(PathComponent {
                    parent_dir: None,
                    name: pipe.get_name(),
                    inode: INode::Pipe(pipe.clone()),
                }),
                options,
                0,
            )
            .into(),
            options,
        )?;

        Ok(pipe)
    }
}
