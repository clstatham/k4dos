use alloc::{borrow::ToOwned, boxed::Box, string::String, sync::Arc};

use super::INode;

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct Path {
    path: str,
}

impl Path {
    pub fn new(path: &str) -> &Path {
        let path = if path == "/" {
            path
        } else {
            path.trim_end_matches('/')
        };
        unsafe { &*(path as *const str as *const Path) }
    }

    pub fn as_str(&self) -> &str {
        &self.path
    }

    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    pub fn is_absolute(&self) -> bool {
        self.path.starts_with('/')
            && !self
                .components()
                .any(|comp| matches!(comp, ".." | "." | ""))
    }

    pub fn is_pipe(&self) -> bool {
        self.path.starts_with("pipe:")
    }

    pub fn pipe_name(&self) -> Option<&str> {
        if self.is_pipe() {
            Some(&self.path[5..])
        } else {
            None
        }
    }

    pub fn components(&self) -> Components<'_> {
        let path = if self.path.starts_with('/') {
            &self.path[1..]
        } else {
            &self.path
        };

        Components { path }
    }

    pub fn parent_and_basename(&self) -> Option<(&Path, &str)> {
        if &self.path == "/" {
            return None;
        }

        if let Some(slash_idx) = self.path.rfind('/') {
            let parent_dir = if slash_idx == 0 {
                Path::new("/")
            } else {
                Path::new(&self.path[..slash_idx])
            };

            let basename = &self.path[(slash_idx + 1)..];
            Some((parent_dir, basename))
        } else {
            Some((Path::new("."), &self.path))
        }
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl core::fmt::Display for Path {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", &self.path)
    }
}

pub struct Components<'a> {
    path: &'a str,
}

impl<'a> Iterator for Components<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<Self::Item> {
        if self.path.is_empty() {
            return None;
        }

        let (path_str, next_start) = match self.path.find('/') {
            Some(slash_idx) => (&self.path[..slash_idx], slash_idx + 1),
            None => (self.path, self.path.len()),
        };

        self.path = &self.path[next_start..];
        Some(path_str)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct PathBuf {
    path: String,
}

impl PathBuf {
    pub fn new() -> PathBuf {
        PathBuf {
            path: String::new(),
        }
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.path)
    }

    pub fn pop(&mut self) {
        if let Some((index, _)) = self.path.char_indices().rfind(|(_, ch)| *ch == '/') {
            self.path.truncate(index);
        }
    }

    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        let path_str = if path.as_str() == "/" {
            "/"
        } else {
            path.as_str().trim_end_matches('/')
        };

        if path.is_absolute() {
            self.path = path_str.to_owned()
        } else {
            if self.path != "/" {
                self.path.push('/');
            }
            self.path.push_str(path_str)
        }
    }
}

impl Default for PathBuf {
    fn default() -> Self {
        PathBuf::new()
    }
}

impl core::ops::Deref for PathBuf {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl From<&Path> for PathBuf {
    fn from(value: &Path) -> Self {
        PathBuf {
            path: value.path.to_owned(),
        }
    }
}

impl From<String> for PathBuf {
    fn from(value: String) -> Self {
        // TODO: check if this is a valid path
        PathBuf { path: value }
    }
}

impl From<&str> for PathBuf {
    fn from(value: &str) -> Self {
        // TODO: check if this is a valid path
        PathBuf {
            path: value.to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct PathComponent {
    pub parent_dir: Option<Box<PathComponent>>,
    pub name: Arc<String>,
    pub inode: INode,
}

impl PathComponent {
    pub fn resolve_abs_path(&self) -> PathBuf {
        let path = if self.parent_dir.is_some() {
            let mut path = self.name.as_ref().to_owned();
            let mut parent_dir = self.parent_dir.clone();
            while let Some(ref path_comp) = parent_dir {
                path = path_comp.name.as_ref().to_owned() + "/" + &path;
                parent_dir = path_comp.parent_dir.clone();
            }

            debug_assert!(path.starts_with('/'));
            path
        } else {
            "/".to_owned()
        };

        PathBuf::from(path)
    }
}
