use alloc::{borrow::ToOwned, string::String, sync::Arc};

use crate::{
    errno,
    fs::{
        path::{Path, PathComponent},
        DirRef, INode,
    },
    util::{errno::Errno, KResult},
};

use super::dir::InitRamFsDir;

const MAX_SYMLINK_FOLLOW_DEPTH: usize = 20;

pub struct RootFs {
    root_path: Arc<PathComponent>,
    cwd_path: Arc<PathComponent>,
}

impl RootFs {
    pub fn new(root: Arc<InitRamFsDir>) -> RootFs {
        let root_path = Arc::new(PathComponent {
            parent_dir: None,
            name: String::new(),
            inode: INode::Dir(root),
        });
        RootFs {
            root_path: root_path.clone(),
            cwd_path: root_path,
        }
    }

    pub fn cwd_path(&self) -> Arc<PathComponent> {
        self.cwd_path.clone()
    }

    pub fn root_dir(&self) -> DirRef {
        self.root_path.inode.as_dir().unwrap().clone()
    }

    pub fn cwd_dir(&self) -> DirRef {
        self.cwd_path.inode.as_dir().unwrap().clone()
    }

    pub fn lookup(&self, path: &Path) -> KResult<INode> {
        self.lookup_path(path, true).map(|cmp| cmp.inode.clone())
    }

    pub fn lookup_path(&self, path: &Path, follow_symlinks: bool) -> KResult<Arc<PathComponent>> {
        if path.is_empty() {
            return Err(errno!(Errno::ENOENT));
        }
        let lookup_from = if path.is_absolute() {
            self.root_path.clone()
        } else {
            self.cwd_path.clone()
        };

        self.do_lookup_path(
            &lookup_from,
            path,
            follow_symlinks,
            MAX_SYMLINK_FOLLOW_DEPTH,
        )
    }

    fn do_lookup_path(
        &self,
        lookup_from: &Arc<PathComponent>,
        path: &Path,
        follow_symlinks: bool,
        symlink_follow_limit: usize,
    ) -> KResult<Arc<PathComponent>> {
        let mut parent = lookup_from.clone();
        let mut components = path.components().peekable();
        while let Some(name) = components.next() {
            let path_comp = match name {
                "." => continue,
                ".." => parent
                    .parent_dir
                    .as_ref()
                    .unwrap_or(&self.root_path)
                    .clone(),
                _ => {
                    let inode = parent.inode.as_dir()?.lookup(name)?;
                    Arc::new(PathComponent {
                        parent_dir: Some(parent.clone()),
                        name: name.to_owned(),
                        inode,
                    })
                }
            };

            if components.peek().is_some() {
                parent = match &path_comp.inode {
                    INode::Dir(_) => path_comp,
                    INode::Symlink(link) if follow_symlinks => {
                        if symlink_follow_limit == 0 {
                            return Err(errno!(Errno::ELOOP));
                        }
                        let dst = link.link_location()?;
                        let follow_from = if dst.is_absolute() {
                            &self.root_path
                        } else {
                            &parent
                        };

                        let dst_path = self.do_lookup_path(
                            follow_from,
                            &dst,
                            follow_symlinks,
                            symlink_follow_limit - 1,
                        )?;

                        match dst_path.inode {
                            INode::Dir(_) => dst_path,
                            _ => return Err(errno!(Errno::ENOTDIR)),
                        }
                    }
                    INode::Symlink(_) => return Err(errno!(Errno::ENOTDIR)),
                    INode::File(_) => return Err(errno!(Errno::ENOTDIR)),
                }
            } else {
                match &path_comp.inode {
                    INode::Symlink(link) if follow_symlinks => {
                        if symlink_follow_limit == 0 {
                            return Err(errno!(Errno::ELOOP));
                        }
                        let dst = link.link_location()?;
                        let follow_from = if dst.is_absolute() {
                            &self.root_path
                        } else {
                            &parent
                        };

                        return self.do_lookup_path(
                            follow_from,
                            &dst,
                            follow_symlinks,
                            symlink_follow_limit - 1,
                        );
                    }
                    _ => return Ok(path_comp),
                }
            }
        }

        Ok(parent)
    }
}
