use alloc::{collections::BTreeMap, string::{String, ToString}, sync::{Weak, Arc}};

use crate::{fs::{INode, alloc_inode_no, Directory, FsNode, Stat, FileMode, S_IFDIR}, util::{lock::{SpinLock}, KResult, errno::Errno}, errno};

use super::file::InitRamFsFile;

pub struct DirInner {
    pub children: BTreeMap<String, INode>,
    pub stat: Stat,
    pub name: String,
}

pub struct InitRamFsDir {
    pub(super) parent: Weak<InitRamFsDir>,
    pub(super) inner: SpinLock<DirInner>,
}

impl InitRamFsDir {
    pub fn new(name: String, inode_no: usize) -> InitRamFsDir {
        InitRamFsDir { parent: Weak::new(), inner: SpinLock::new(DirInner {
            name,
            children: BTreeMap::new(),
            stat: Stat {
                inode_no,
                mode: FileMode::new(S_IFDIR | 0o755),
                ..Stat::zeroed()
            }
        }) }
    }

    pub fn add_dir(&self, name: String) -> Arc<InitRamFsDir> {
        let dir = Arc::new(InitRamFsDir::new(name.clone(), alloc_inode_no()));
        // self.inner.with_write(|inner| {
        self.inner.lock().children.insert(name, INode::Dir(dir.clone()));
        // });
        dir
    }

    pub fn add_file(&self, name: String) -> Arc<InitRamFsFile> {
        let file = Arc::new(InitRamFsFile::new(name.clone(), alloc_inode_no()));
        self.inner.lock().children.insert(name, INode::File(file.clone()));
        file
    }

    pub fn parent_dir(&self) -> Option<Arc<InitRamFsDir>> {
        self.parent.upgrade()
    }
}

impl Directory for InitRamFsDir {
    fn insert(&self, name: &str, inode: INode) {
        self.inner.lock().children.insert(name.to_string(), inode);
    }

    fn lookup(&self, name: &str) -> KResult<INode> {
        let inode = self.inner.lock().children.get(name).cloned().ok_or(errno!(Errno::ENOENT))?;
        Ok(inode)
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(self.inner.lock().stat)
    }
}

impl FsNode for InitRamFsDir {
    fn get_name(&self) -> String {
        self.inner.lock().name.clone()
    }
}
