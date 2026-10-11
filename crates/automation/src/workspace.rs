//! File access for agents. An MCP client may read only below the read root and write only below
//! the write root it was given at launch; without a root it has no access of that kind.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Workspace {
    read_root: Option<PathBuf>,
    write_root: Option<PathBuf>,
}

impl Workspace {
    /// Both roots must exist; they are resolved to absolute paths now, so later checks compare
    /// real locations.
    pub fn new(read_root: Option<&Path>, write_root: Option<&Path>) -> Result<Self, String> {
        let resolve = |root: Option<&Path>, what: &str| -> Result<Option<PathBuf>, String> {
            root.map(|r| r.canonicalize().map_err(|e| format!("{what} root {}: {e}", r.display()))).transpose()
        };
        Ok(Self { read_root: resolve(read_root, "read")?, write_root: resolve(write_root, "write")? })
    }

    /// An existing file below the read root. `path` is relative to the root, or absolute and
    /// inside it; symlinks that lead outside are refused.
    pub fn readable(&self, path: &str) -> Result<PathBuf, String> {
        let root = self.read_root.as_ref().ok_or("no read root was granted (start the server with --read-root)")?;
        let candidate = join(root, path)?;
        let real = candidate.canonicalize().map_err(|e| format!("{path}: {e}"))?;
        if !real.starts_with(root) {
            return Err(format!("{path} is outside the read root"));
        }
        if !real.is_file() {
            return Err(format!("{path} is not a file"));
        }
        Ok(real)
    }

    /// A file to create below the write root. Its folder must exist inside the root; an existing
    /// file is replaced only when `overwrite` is set, and never through a symlink.
    pub fn writable(&self, path: &str, overwrite: bool) -> Result<PathBuf, String> {
        let root = self.write_root.as_ref().ok_or("no write root was granted (start the server with --write-root)")?;
        let candidate = join(root, path)?;
        let name = candidate.file_name().ok_or_else(|| format!("{path} has no file name"))?.to_owned();
        let parent = candidate.parent().ok_or_else(|| format!("{path} has no folder"))?;
        let real_parent = parent.canonicalize().map_err(|e| format!("{path}: folder: {e}"))?;
        if !real_parent.starts_with(root) {
            return Err(format!("{path} is outside the write root"));
        }
        let target = real_parent.join(name);
        match std::fs::symlink_metadata(&target) {
            Ok(meta) if meta.file_type().is_symlink() => {
                Err(format!("{path} is a symlink; refusing to write through it"))
            }
            Ok(meta) if meta.is_dir() => Err(format!("{path} is a folder")),
            Ok(_) if !overwrite => Err(format!("{path} already exists; pass overwrite: true to replace it")),
            _ => Ok(target),
        }
    }
}

/// A path for people to read: Windows' verbatim prefix (`\\?\`), which canonicalising adds, is
/// dropped.
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}

/// Joins `path` to `root`, refusing `..` and, for absolute paths, anything outside the root.
fn join(root: &Path, path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path);
    if path.is_empty() {
        return Err("path is empty".into());
    }
    if given.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{path}: '..' is not allowed"));
    }
    if given.is_absolute() {
        if !given.starts_with(root) {
            return Err(format!("{path} is outside the granted root"));
        }
        return Ok(given.to_path_buf());
    }
    Ok(root.join(given))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loopwerk-workspace-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        std::fs::write(dir.join("inside").join("a.als"), b"x").unwrap();
        std::fs::write(dir.join("outside.als"), b"x").unwrap();
        dir
    }

    #[test]
    fn display_path_drops_the_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\a\b.wav")), r"C:\a\b.wav");
        assert_eq!(display_path(Path::new("/tmp/b.wav")), "/tmp/b.wav");
    }

    #[test]
    fn reads_only_below_the_read_root() {
        let dir = temp_dir("read");
        let ws = Workspace::new(Some(&dir.join("inside")), None).unwrap();
        assert!(ws.readable("a.als").is_ok());
        assert!(ws.readable("../outside.als").is_err());
        assert!(ws.readable(dir.join("outside.als").to_str().unwrap()).is_err());
        assert!(ws.readable("missing.als").is_err());
        assert!(ws.readable("").is_err());
        assert!(ws.writable("x.wav", false).is_err(), "no write root granted");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_only_below_the_write_root_and_never_overwrites_by_accident() {
        let dir = temp_dir("write");
        let ws = Workspace::new(None, Some(&dir.join("inside"))).unwrap();
        assert!(ws.writable("new.wav", false).is_ok());
        assert!(ws.writable("a.als", false).is_err());
        assert!(ws.writable("a.als", true).is_ok());
        assert!(ws.writable("../escape.wav", false).is_err());
        assert!(ws.writable("missing-folder/x.wav", false).is_err());
        assert!(ws.readable("a.als").is_err(), "no read root granted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
