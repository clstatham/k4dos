use alloc::string::String;

use crate::{
    fs::{path::PathBuf, FsNode, Stat, Symlink},
    util::KResult,
};

pub struct InitRamFsSymlink {
    pub(crate) name: String,
    pub(crate) dst: PathBuf,
    pub(crate) stat: Stat,
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
