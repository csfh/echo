//! In-memory Echo sources for hosts that have no real filesystem.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Logical Echo sources keyed by Unix-style paths.
///
/// Paths are normalized (`a/../b` → `b`). There is no disk I/O.
#[derive(Debug, Clone, Default)]
pub struct VirtualSources {
    files: HashMap<PathBuf, String>,
}

impl VirtualSources {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl AsRef<Path>, text: impl Into<String>) {
        self.files
            .insert(normalize_path(path.as_ref()), text.into());
    }

    #[must_use]
    pub fn get(&self, path: &Path) -> Option<&str> {
        self.files.get(&normalize_path(path)).map(String::as_str)
    }

    #[must_use]
    pub fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(&normalize_path(path))
    }

    /// True when any stored file lives strictly under `path`.
    #[must_use]
    pub fn is_dir(&self, path: &Path) -> bool {
        let dir = normalize_path(path);
        if dir.as_os_str().is_empty() {
            return !self.files.is_empty();
        }
        self.files
            .keys()
            .any(|file| file.starts_with(&dir) && file.as_os_str() != dir.as_os_str())
    }

    /// Direct `*.echo` children of `dir` (not recursive). Matches disk
    /// [`crate::list_echo_files`].
    #[must_use]
    pub fn list_echo_files(&self, dir: &Path) -> Vec<PathBuf> {
        let dir = normalize_path(dir);
        let mut files: Vec<PathBuf> = self
            .files
            .keys()
            .filter(|p| {
                p.parent() == Some(dir.as_path())
                    && p.extension().and_then(|x| x.to_str()) == Some("echo")
            })
            .cloned()
            .collect();
        files.sort();
        files
    }

    /// Prefer `base.echo`, else a directory that contains `*.echo`.
    pub fn resolve_file_or_dir_module(&self, base: &Path) -> Result<PathBuf, String> {
        let base = normalize_path(base);
        let file = base.with_extension("echo");
        if self.is_file(&file) {
            return Ok(file);
        }
        if self.is_dir(&base) {
            let echoes = self.list_echo_files(&base);
            if !echoes.is_empty() {
                return Ok(base);
            }
            return Err(format!("directory {} has no .echo sources", base.display()));
        }
        Err(format!(
            "no module at {}.echo or {}/",
            base.display(),
            base.display()
        ))
    }
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_and_folder_lookup() {
        let mut src = VirtualSources::new();
        src.insert("/echo/std/io.echo", "/ runtime\n");
        src.insert("/echo/std/net/tcp/conn.echo", "\\ conn\n");
        src.insert("/echo/std/net/tcp/listener.echo", "\\ listener\n");
        src.insert("/echo/playground.echo", "/ std/io\n");

        assert!(src.is_file(Path::new("/echo/std/io.echo")));
        assert!(src.is_dir(Path::new("/echo/std")));
        assert!(src.is_dir(Path::new("/echo/std/net/tcp")));
        assert!(!src.is_dir(Path::new("/echo/std/io.echo")));
        assert!(!src.is_file(Path::new("/echo/stdio.echo")));

        let tcp = src
            .resolve_file_or_dir_module(Path::new("/echo/std/net/tcp"))
            .unwrap();
        assert_eq!(tcp, PathBuf::from("/echo/std/net/tcp"));
        let listed = src.list_echo_files(&tcp);
        assert_eq!(listed.len(), 2);

        let io = src
            .resolve_file_or_dir_module(Path::new("/echo/std/io"))
            .unwrap();
        assert_eq!(io, PathBuf::from("/echo/std/io.echo"));
    }
}
