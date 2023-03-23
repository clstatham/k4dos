use core::iter::Peekable;

use alloc::{
    string::{String, ToString},
    sync::{Arc, Weak}, vec::Vec,
};
use spin::Once;

use crate::{
    errno,
    fs::{
        initramfs::{
            dir::{DirInner, InitRamFsDir},
            file::InitRamFsFile,
            symlink::InitRamFsSymlink,
        },
        path::{Components, Path, PathBuf},
        DirRef, FileMode, FileSize, FsNode, INode, Stat,
    },
    util::{align_up, errno::Errno, KResult, IrqMutex},
};

use self::root::RootFs;

pub mod dir;
pub mod file;
pub mod root;
pub mod symlink;

pub struct ByteParser<'a> {
    buffer: &'a [u8],
    current: usize,
}

impl<'a> ByteParser<'a> {
    pub fn new(buffer: &'a [u8]) -> ByteParser<'a> {
        ByteParser { buffer, current: 0 }
    }

    pub fn remaining(&self) -> &[u8] {
        &self.buffer[self.current..]
    }
    pub fn remaining_len(&self) -> usize {
        self.buffer.len() - self.current
    }

    pub fn skip(&mut self, len: usize) -> KResult<()> {
        if self.current + len > self.buffer.len() {
            return Err(errno!(Errno::EINVAL));
        }

        self.current += len;
        Ok(())
    }

    pub fn skip_until_alignment(&mut self, align: usize) -> KResult<()> {
        let next = align_up(self.current, align);
        if next > self.buffer.len() {
            return Err(errno!(Errno::EINVAL));
        }

        self.current = next;
        Ok(())
    }

    pub fn consume_bytes(&mut self, len: usize) -> KResult<&'a [u8]> {
        if self.current + len > self.buffer.len() {
            return Err(errno!(Errno::EINVAL));
        }

        self.current += len;
        Ok(&self.buffer[self.current - len..self.current])
    }
}

fn parse_str_field(bytes: &[u8]) -> KResult<&str> {
    core::str::from_utf8(bytes).map_err(|_e| errno!(Errno::EINVAL))
}

fn parse_hex_field(bytes: &[u8]) -> KResult<usize> {
    usize::from_str_radix(parse_str_field(bytes)?, 16).map_err(|_e| errno!(Errno::EINVAL))
}

pub static INITRAM_FS: Once<Arc<InitRamFs>> = Once::new();

pub fn init() -> KResult<()> {
    INITRAM_FS.call_once(|| {
        let image = include_bytes!("../../../initramfs/initramfs");
        if image.is_empty() {
            panic!("initramfs not embedded");
        }

        log::info!("Parsing initramfs...");
        Arc::new(InitRamFs::parse(image.as_slice()).expect("error parsing initramfs"))
    });
    Ok(())
}

pub fn get_root() -> Option<&'static RootFs> {
    Some(&INITRAM_FS.get()?.root)
}

pub struct InitRamFs {
    root: RootFs,
}

impl InitRamFs {
    pub fn parse(fs_image: &[u8]) -> KResult<InitRamFs> {
        let mut image = ByteParser::new(fs_image);
        let mut n_files = 0;
        let mut loaded_size = 0;
        let root = Arc::new(InitRamFsDir::new(String::new(), 2));
        loop {
            if image.remaining_len() == 0 {
                break;
            }
            let magic = image.consume_bytes(6).and_then(parse_hex_field)?;
            if magic != 0x070701 {
                log::error!(
                    "initramfs: invalid magic (expected {:#x}, got {:#x})",
                    0x070701,
                    magic
                );
                return Err(errno!(Errno::EINVAL));
            }

            let ino = parse_hex_field(image.consume_bytes(8)?)?;
            let mode = FileMode::new(parse_hex_field(image.consume_bytes(8)?)? as u32);
            let _uid = parse_hex_field(image.consume_bytes(8)?)?;
            let _gid = parse_hex_field(image.consume_bytes(8)?)?;
            let _nlink = parse_hex_field(image.consume_bytes(8)?)?;
            let _mtime = parse_hex_field(image.consume_bytes(8)?)?;
            let filesize = parse_hex_field(image.consume_bytes(8)?)?;
            let _dev_major = parse_hex_field(image.consume_bytes(8)?)?;
            let _dev_minor = parse_hex_field(image.consume_bytes(8)?)?;

            image.skip(16)?;

            let path_len = parse_hex_field(image.consume_bytes(8)?)?;
            if path_len == 0 {
                return Err(errno!(Errno::EINVAL));
            }

            image.skip(8)?;

            let mut path = parse_str_field(image.consume_bytes(path_len - 1)?)?;

            if path.starts_with("./") {
                path = &path[1..];
            }
            if path == "TRAILER!!!" {
                break;
            }

            if path.is_empty() {
                return Err(errno!(Errno::EINVAL));
                // image.skip(1)?;
                // image.skip_until_alignment(4)?;
                // continue;
            }
            // log::trace!("initramfs: {} ({} bytes)", path, filesize);
            image.skip(1)?;
            image.skip_until_alignment(4)?;

            let components = Path::new(path).components().peekable();

            fn walk(
                mut components_peekable: Peekable<Components>,
                dir: DirRef,
            ) -> Option<(DirRef, String)> {
                // let mut components_peekable = components.peekable();
                let next = components_peekable.next();
                next?;
                if components_peekable.peek().is_none() {
                    return Some((dir, next.unwrap().to_string()));
                }

                let dir_clone = dir.clone();
                if let Ok(child) = dir.lookup(next.unwrap()) {
                    if let INode::Dir(next_dir) = child {
                        return walk(components_peekable, next_dir.clone());
                    } else {
                        return Some((dir_clone, child.get_name()));
                    }
                }
                None
            }

            if path == "." {
                image.skip_until_alignment(4)?;
                continue;
            }

            let walk_result = walk(components, root.clone());
            let (parent_dir, filename) = if let Some((parent, name)) = walk_result {
                (parent, name)
            } else {
                image.consume_bytes(filesize)?;
                image.skip_until_alignment(4)?;
                continue;
            };

            let data = image.consume_bytes(filesize)?;
            if mode.is_symbolic_link() {
                let inode = INode::Symlink(Arc::new(InitRamFsSymlink {
                    name: filename.clone(),
                    dst: PathBuf::from(core::str::from_utf8(data).unwrap()),
                    stat: Stat {
                        inode_no: ino,
                        mode,
                        ..Stat::zeroed()
                    },
                }));
                // parent_dir.with_write(|d| d.insert(&filename, inode));
                parent_dir.insert(inode);
            } else if mode.is_directory() {
                let inode = INode::Dir(Arc::new(InitRamFsDir {
                    parent: Weak::new(),
                    inner: IrqMutex::new(DirInner {
                        children: Vec::new(),
                        stat: Stat {
                            inode_no: ino,
                            mode,
                            ..Stat::zeroed()
                        },
                        name: filename.clone(),
                    }),
                }));
                parent_dir.insert(inode);
            } else if mode.is_regular_file() {
                let file = InitRamFsFile {
                    name: IrqMutex::new(filename.clone()),
                    data: IrqMutex::new(data.to_vec()),
                    stat: IrqMutex::new(Stat {
                        inode_no: ino,
                        mode,
                        size: FileSize(filesize as isize),
                        ..Stat::zeroed()
                    }),
                };
                // file.write(0, UserBuffer::from_slice(data), OpenOptions::empty())?;
                // parent_dir.with_write(|d| d.insert(&filename, INode::File(Arc::new(file))));
                parent_dir.insert(INode::File(Arc::new(file)));
            }

            image.skip_until_alignment(4)?;
            n_files += 1;
            loaded_size += data.len();
        }

        log::info!(
            "initramfs: found {} files taking up {} bytes",
            n_files,
            loaded_size
        );

        Ok(InitRamFs {
            root: RootFs::new(root),
        })
    }
}
