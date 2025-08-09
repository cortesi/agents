use crate::error::Error;
use std::path::{Path, PathBuf};

fn has_vcs_dir(dir: &Path) -> bool {
    dir.join(".git").is_dir() || dir.join(".hg").is_dir() || dir.join(".svn").is_dir()
}

/// Find the project root by walking upwards from `path`.
/// Starts at `path` (or its parent if `path` is a file) and returns the
/// nearest ancestor directory that contains a version control directory
/// (one of `.git`, `.hg`, or `.svn`). If no VCS dir is found, returns the
/// nearest ancestor containing a `Cargo.lock`. Otherwise errors.
pub fn project_root<P: AsRef<Path>>(path: P) -> Result<PathBuf, Error> {
    let mut start = path.as_ref();

    // If `path` is a file, start from its parent.
    if start.is_file()
        && let Some(parent) = start.parent()
    {
        start = parent;
    }

    // Walk up the directory tree preferring VCS roots; remember nearest Cargo.lock.
    let mut cur = Some(start);
    let mut cargo_lock_candidate: Option<PathBuf> = None;
    while let Some(dir) = cur {
        if has_vcs_dir(dir) {
            return Ok(dir.to_path_buf());
        }
        if dir.join("Cargo.lock").is_file() && cargo_lock_candidate.is_none() {
            cargo_lock_candidate = Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }

    if let Some(dir) = cargo_lock_candidate {
        return Ok(dir);
    }

    Err(Error::Root("project root not found".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn detects_by_git_dir() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        let nested_dir = root.join("a/b/c");
        fs::create_dir_all(&nested_dir).unwrap();
        let nested_file = nested_dir.join("file.txt");
        let mut f = fs::File::create(&nested_file).unwrap();
        writeln!(f, "hi").unwrap();

        let found = project_root(&nested_file).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn detects_by_marker_dir() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        let nested = root.join("child");
        fs::create_dir_all(&nested).unwrap();
        let found = project_root(&nested).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn errors_when_not_found() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        let nested = root.join("child");
        fs::create_dir_all(&nested).unwrap();
        let err = project_root(&nested).unwrap_err();
        match err {
            Error::Root(msg) => assert!(msg.contains("not found")),
            _ => panic!("unexpected error variant"),
        }
    }

    #[test]
    fn detects_by_cargo_lock_when_no_vcs() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        // No VCS dirs
        let nested = root.join("x/y");
        fs::create_dir_all(&nested).unwrap();
        // Create Cargo.lock at root
        fs::write(root.join("Cargo.lock"), "").unwrap();
        let found = project_root(&nested).unwrap();
        assert_eq!(found, root);
    }

    #[test]
    fn prefers_vcs_over_cargo_lock() {
        let td = TempDir::new().unwrap();
        let root = td.path();
        // Both markers exist
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join("Cargo.lock"), "").unwrap();
        let nested = root.join("child");
        fs::create_dir_all(&nested).unwrap();
        let found = project_root(&nested).unwrap();
        assert_eq!(found, root);
    }
}
