use alloc::{borrow::ToOwned, string::String, sync::Arc};

use crate::{
    errno,
    fs::{
        path::{Path, PathComponent},
        DirRef, INode, pipe::PIPE_FS,
    },
    util::{errno::Errno, KResult},
};

use super::dir::InitRamFsDir;

const MAX_SYMLINK_FOLLOW_DEPTH: usize = 20;


#[derive(Clone)]
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

    pub fn chdir(&mut self, path: &Path) -> KResult<()> {
        self.cwd_path = self.lookup_path(path, true)?;
        Ok(())
    }

    pub fn lookup(&self, path: &Path, follow_symlinks: bool) -> KResult<INode> {
        if path.is_pipe() {
            return PIPE_FS.lookup(path).map(|pipe| INode::Pipe(pipe));
        }
        self.lookup_path(path, follow_symlinks).map(|cmp| cmp.inode.clone())
    }

    pub fn lookup_path(&self, path: &Path, follow_symlinks: bool) -> KResult<Arc<PathComponent>> {
        if path.is_empty() {
            return Err(errno!(Errno::ENOENT, "lookup_path(): not found"));
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
                    INode::Pipe(_) => unreachable!("Pipes should be contained in PipeFs, not RootFs"),
                    INode::Symlink(link) if follow_symlinks => {
                        if symlink_follow_limit == 0 {
                            return Err(errno!(Errno::ELOOP, "lookup_path(): maximum symlink depth reached"));
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
                            _ => return Err(errno!(Errno::ENOTDIR, "lookup_path(): not a directory")),
                        }
                    }
                    INode::Symlink(_) => return Err(errno!(Errno::ENOTDIR, "lookup_path(): not a directory")),
                    INode::File(_) => return Err(errno!(Errno::ENOTDIR, "lookup_path(): not a directory")),
                }
            } else {
                match &path_comp.inode {
                    INode::Symlink(link) if follow_symlinks => {
                        if symlink_follow_limit == 0 {
                            return Err(errno!(Errno::ELOOP, "lookup_path(): maximum symlink depth reached"));
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
