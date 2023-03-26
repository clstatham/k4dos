use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::{string::String, sync::Arc};
use bitflags::bitflags;

use crate::{
    errno,
    task::wait_queue::WaitQueue,
    userland::buffer::{UserBuffer, UserBufferMut},
    util::{ctypes::c_short, errno::Errno, KResult},
};

use self::{opened_file::OpenFlags, path::PathBuf, pipe::Pipe};

pub mod initramfs;
pub mod opened_file;
pub mod path;
pub mod pipe;
pub mod devfs;

pub type FileRef = Arc<dyn File + Send + Sync>;
pub type DirRef = Arc<dyn Directory + Send + Sync>;
pub type SymlinkRef = Arc<dyn Symlink + Send + Sync>;

pub static POLL_WAIT_QUEUE: WaitQueue = WaitQueue::new();

pub fn alloc_inode_no() -> usize {
    // Inode #1 is reserved for the root dir.
    static NEXT_INODE_NO: AtomicUsize = AtomicUsize::new(2);

    NEXT_INODE_NO.fetch_add(1, Ordering::AcqRel)
}

bitflags! {
    #[derive(Debug)]
    pub struct PollStatus: c_short {
        const POLLIN     = 0x001;
        const POLLPRI    = 0x002;
        const POLLOUT    = 0x004;
        const POLLERR    = 0x008;
        const POLLHUP    = 0x010;
        const POLLNVAL   = 0x020;
        const POLLRDNORM = 0x040;
        const POLLRDBAND = 0x080;
        const POLLWRNORM = 0x100;
        const POLLWRBAND = 0x200;
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
#[non_exhaustive]
pub enum FileType {
    Directory = 4,
    Regular = 8,
    Link = 10,
}

/// for readdir(3)
pub struct DirEntry {
    pub inode_no: usize,
    pub file_type: FileType,
    pub name: String,
}

/// The device file's ID.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct DevId(usize);

/// The number of hard links.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct NLink(usize);

/// The file size in bytes.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct FileSize(pub isize);

/// The user ID.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct UId(u32);

/// The Group ID.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct GId(u32);

/// The size in bytes of a block file file system I/O operations.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct BlockSize(isize);

/// The number of blocks.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct BlockCount(isize);

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct Time(isize);

#[derive(Debug, Copy, Clone)]
#[repr(C, packed)]
pub struct Stat {
    pub dev: DevId,
    pub inode_no: usize,
    pub nlink: NLink,
    pub mode: FileMode,
    pub uid: UId,
    pub gid: GId,
    pub pad0: u32,
    pub rdev: DevId,
    pub size: FileSize,
    pub blksize: BlockSize,
    pub blocks: BlockCount,
    pub atime: Time,
    pub mtime: Time,
    pub ctime: Time,
}

impl Stat {
    pub fn zeroed() -> Stat {
        Stat {
            dev: DevId(0),
            inode_no: 0,
            mode: FileMode(0),
            nlink: NLink(0),
            uid: UId(0),
            gid: GId(0),
            pad0: 0,
            rdev: DevId(0),
            size: FileSize(0),
            blksize: BlockSize(0),
            blocks: BlockCount(0),
            atime: Time(0),
            mtime: Time(0),
            ctime: Time(0),
        }
    }
}

pub const S_IFMT: u32 = 0o170000;
pub const S_IFCHR: u32 = 0o020000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFLNK: u32 = 0o120000;

pub const O_ACCMODE: u32 = 0o3;

// FIXME: OpenFlags also define these values.
#[allow(unused)]
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct FileMode(u32);

impl FileMode {
    pub fn new(val: u32) -> FileMode {
        FileMode(val)
    }

    pub fn access_mode(self) -> u32 {
        self.0 & O_ACCMODE
    }

    pub fn is_directory(self) -> bool {
        (self.0 & S_IFMT) == S_IFDIR
    }

    pub fn is_regular_file(self) -> bool {
        (self.0 & S_IFMT) == S_IFREG
    }

    pub fn is_symbolic_link(self) -> bool {
        (self.0 & S_IFMT) == S_IFLNK
    }
}

pub trait FsNode {
    fn get_name(&self) -> String;
}

pub trait File: FsNode {
    /// `open(2)`.
    fn open(&self, _options: &OpenFlags) -> KResult<Option<FileRef>> {
        Ok(None)
    }

    /// `stat(2)`.
    fn stat(&self) -> KResult<Stat> {
        Err(errno!(Errno::EBADF, "stat(): not implemented"))
    }

    /// `readlink(2)`.
    fn readlink(&self) -> KResult<PathBuf> {
        // "EINVAL - The named file is not a symbolic link." -- readlink(2)
        Err(errno!(Errno::EINVAL, "not a symbolic link"))
    }

    /// `poll(2)` and `select(2)`.
    fn poll(&self) -> KResult<PollStatus> {
        Err(errno!(Errno::EBADF, "poll(): not implemented"))
    }

    /// `ioctl(2)`.
    fn ioctl(&self, _cmd: usize, _arg: usize) -> KResult<isize> {
        Err(errno!(Errno::EBADF, "ioctl(): not implemented"))
    }

    /// `read(2)`.
    fn read(&self, _offset: usize, _buf: UserBufferMut, _options: &OpenFlags) -> KResult<usize> {
        Err(errno!(Errno::EBADF, "read(): not implemented"))
    }

    /// `write(2)`.
    fn write(
        &self,
        _offset: usize,
        _buf: UserBuffer<'_>,
        _options: &OpenFlags,
    ) -> KResult<usize> {
        Err(errno!(Errno::EBADF, "write(): not implemented"))
    }
}

pub trait Symlink: FsNode {
    fn link_location(&self) -> KResult<PathBuf>;
    fn stat(&self) -> KResult<Stat>;
    fn fsync(&self) -> KResult<()> {
        Ok(())
    }
}

pub trait Directory: FsNode {
    fn insert(&self, inode: INode);

    /// Looks for an existing file.
    fn lookup(&self, name: &str) -> KResult<INode>;
    /// `stat(2)`.
    fn stat(&self) -> KResult<Stat>;
    /// `fsync(2)`.
    fn fsync(&self) -> KResult<()> {
        Ok(())
    }
    /// `readlink(2)`.
    fn readlink(&self) -> KResult<PathBuf> {
        // "EINVAL - The named file is not a symbolic link." -- readlink(2)
        Err(errno!(Errno::EINVAL, "readlink(): not a symbolic link"))
    }

    fn readdir(&self, index: usize) -> KResult<Option<DirEntry>>;

    fn unlink(&self, name: String) -> KResult<()>;
}

#[derive(Clone)]
pub enum INode {
    File(FileRef),
    Dir(DirRef),
    Symlink(SymlinkRef),
    Pipe(Arc<Pipe>),
}

impl INode {
    pub fn is_file(&self) -> bool {
        matches!(self, INode::File(_))
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, INode::Dir(_))
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self, INode::Symlink(_))
    }

    pub fn is_pipe(&self) -> bool {
        matches!(self, INode::Pipe(_))
    }

    pub fn as_file(&self) -> KResult<&FileRef> {
        match self {
            INode::File(file) => Ok(file),
            _ => Err(errno!(Errno::EINVAL, "as_file(): not a file")),
        }
    }

    pub fn stat(&self) -> KResult<Stat> {
        match self {
            INode::Dir(d) => d.stat(),
            INode::File(d) => d.stat(),
            INode::Symlink(d) => d.stat(),
            INode::Pipe(p) => p.stat(),
        }
    }

    pub fn as_dir(&self) -> KResult<&DirRef> {
        match self {
            INode::Dir(d) => Ok(d),
            _ => Err(errno!(Errno::EINVAL, "as_dir(): not a directory")),
        }
    }

    pub fn as_symlink(&self) -> KResult<&SymlinkRef> {
        match self {
            INode::Symlink(s) => Ok(s),
            _ => Err(errno!(Errno::EINVAL, "as_symlink(): not a symbolic link")),
        }
    }

    pub fn as_pipe(&self) -> KResult<&Arc<Pipe>> {
        match self {
            INode::Pipe(p) => Ok(p),
            _ => Err(errno!(Errno::EINVAL, "as_pipe(): not a pipe")),
        }
    }
}

impl FsNode for INode {
    fn get_name(&self) -> String {
        match self {
            INode::Dir(d) => d.get_name(),
            INode::File(f) => f.get_name(),
            INode::Symlink(l) => l.get_name(),
            INode::Pipe(p) => p.get_name(),
        }
    }
}
