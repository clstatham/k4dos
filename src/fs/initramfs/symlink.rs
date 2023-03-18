use alloc::string::String;

use crate::{fs::{path::PathBuf, Symlink, FsNode, Stat}, util::KResult};

pub struct InitRamFsSymlink {
    pub(super) name: String,
    pub(super) dst: PathBuf,
    pub(super) stat: Stat,
}

impl Symlink for InitRamFsSymlink {
    fn link_location(&self) -> KResult<PathBuf> {
        Ok(self.dst.clone())
    }

    fn stat(&self) -> KResult<Stat> {
        Ok(self.stat)
    }
}

impl FsNode for InitRamFsSymlink {
    fn get_name(&self) -> String {
        self.name.clone()
    }
    
}
