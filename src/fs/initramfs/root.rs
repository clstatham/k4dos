use alloc::{borrow::ToOwned, boxed::Box, string::String, sync::Arc};

use crate::{
    fs::{
        path::{Path, PathComponent},
        pipe::PIPE_FS,
        DirRef, INode,
    },
    kbail,
    util::KResult,
};

use super::dir::InitRamFsDir;

const MAX_SYMLINK_FOLLOW_DEPTH: usize = 20;

#[derive(Clone)]
pub struct RootFs {
    root_path: PathComponent,
    cwd_path: PathComponent,
}

impl RootFs {
    pub fn new(root: Arc<InitRamFsDir>) -> RootFs {
        let root_path = PathComponent {
            parent_dir: None,
            name: Arc::new(String::new()),
            inode: INode::Dir(root),
        };
        RootFs {
            root_path: root_path.clone(),
            cwd_path: root_path,
        }
    }

    pub fn cwd_path(&self) -> &PathComponent {
        &self.cwd_path
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
            return PIPE_FS.lookup(path).map(INode::Pipe);
        }
        self.lookup_path(path, follow_symlinks)
            .map(|cmp| cmp.inode.clone())
    }

    pub fn lookup_path(&self, path: &Path, follow_symlinks: bool) -> KResult<PathComponent> {
        if path.is_empty() {
            kbail!(ENOENT, "lookup_path(): empty path");
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
        lookup_from: &PathComponent,
        path: &Path,
        follow_symlinks: bool,
        symlink_follow_limit: usize,
    ) -> KResult<PathComponent> {
        let mut parent = lookup_from.clone();
        let mut components = path.components().peekable();
        while let Some(name) = components.next() {
            let path_comp = match name {
                "." => continue,
                ".." => parent
                    .parent_dir
                    .as_deref()
                    .unwrap_or(&self.root_path)
                    .clone(),
                _ => {
                    let inode = parent.inode.as_dir()?.lookup(name)?;
                    PathComponent {
                        parent_dir: Some(Box::new(parent.clone())),
                        name: Arc::new(name.to_owned()),
                        inode,
                    }
                }
            };

            if components.peek().is_some() {
                parent = match &path_comp.inode {
                    INode::Dir(_) => path_comp,
                    INode::Pipe(_) => {
                        unreachable!("Pipes should be contained in PipeFs, not RootFs")
                    }
                    INode::Symlink(link) if follow_symlinks => {
                        if symlink_follow_limit == 0 {
                            kbail!(ELOOP, "lookup_path(): maximum symlink depth reached");
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
                            _ => {
                                kbail!(ENOTDIR, "lookup_path(): not a directory");
                            }
                        }
                    }
                    INode::Symlink(_) => {
                        kbail!(ENOTDIR, "lookup_path(): not a directory");
                    }
                    INode::File(_) => {
                        kbail!(ENOTDIR, "lookup_path(): not a directory");
                    }
                }
            } else {
                match &path_comp.inode {
                    INode::Symlink(link) if follow_symlinks => {
                        if symlink_follow_limit == 0 {
                            kbail!(ELOOP, "lookup_path(): maximum symlink depth reached");
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
