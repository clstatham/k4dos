use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{
    fs::{
        alloc_inode_no, DirEntry, Directory, FileMode, FileRef, FileType, FsNode, INode, Stat,
        S_IFDIR,
    },
    kerror,
    util::{lock::IrqMutex, KResult},
};

pub struct DirInner {
    pub children: Vec<INode>,
    pub stat: Stat,
    pub name: String,
}

pub struct InitRamFsDir {
    pub(super) parent: Weak<InitRamFsDir>,
    pub(super) inner: IrqMutex<DirInner>,
}

impl InitRamFsDir {
    pub fn new(name: String, inode_no: usize) -> InitRamFsDir {
        InitRamFsDir {
            parent: Weak::new(),
            inner: IrqMutex::new(DirInner {
                name,
                children: Vec::new(),
                stat: Stat {
                    inode_no,
                    mode: FileMode::new(S_IFDIR | 0o755),
                    ..Stat::zeroed()
                },
            }),
        }
    }

    pub fn add_dir(&self, name: String) -> Arc<InitRamFsDir> {
        let dir = Arc::new(InitRamFsDir::new(name, alloc_inode_no()));
        self.inner.lock().children.push(INode::Dir(dir.clone()));
        dir
    }

    pub fn add_file(&self, file: FileRef) {
        self.inner.lock().children.push(INode::File(file.clone()));
    }

    pub fn parent_dir(&self) -> Option<Arc<InitRamFsDir>> {
        self.parent.upgrade()
    }
}

impl Directory for InitRamFsDir {
    fn insert(&self, inode: INode) {
        self.inner.lock().children.push(inode);
    }

    fn lookup(&self, name: &str) -> KResult<INode> {
        let inode = self
            .inner
            .lock()
            .children
            .iter()
            .find(|child| child.get_name() == *name)
            .cloned()
            .ok_or(kerror!(ENOENT, "lookup(): not found"))?;
        Ok(inode)
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(self.inner.lock().stat)
    }

    fn readdir(&self, index: usize) -> KResult<Option<crate::fs::DirEntry>> {
        let entry = self
            .inner
            .lock()
            .children
            .get(index)
            .map(|entry| match entry {
                INode::Pipe(_) => unreachable!("Pipes should be in PipeFs"),
                INode::Dir(dir) => DirEntry {
                    inode_no: dir.stat().unwrap().inode_no,
                    file_type: FileType::Directory,
                    name: dir.get_name(),
                },
                INode::File(file) => DirEntry {
                    inode_no: file.stat().unwrap().inode_no,
                    file_type: FileType::Directory,
                    name: file.get_name(),
                },
                INode::Symlink(link) => DirEntry {
                    inode_no: link.stat().unwrap().inode_no,
                    file_type: FileType::Link,
                    name: link.get_name(),
                },
            });

        Ok(entry)
    }

    fn unlink(&self, name: &str) -> KResult<()> {
        self.inner
            .lock()
            .children
            .retain(|child| child.get_name() != name);
        Ok(())
    }
}

impl FsNode for InitRamFsDir {
    fn get_name(&self) -> String {
        self.inner.lock().name.clone()
    }
}
