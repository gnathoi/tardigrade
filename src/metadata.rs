use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::error::{Error, Result};
use crate::format::{FileEntry, FileType};

/// Capture metadata from a filesystem path into a FileEntry (without block_refs).
pub fn capture_metadata(path: &Path, base: &Path) -> Result<FileEntry> {
    let meta = fs::symlink_metadata(path).map_err(|e| Error::io_path(path, e))?;

    let relative = path
        .strip_prefix(base)
        .unwrap_or(path);

    // Convert path to raw bytes
    #[cfg(unix)]
    let path_bytes = {
        use std::os::unix::ffi::OsStrExt;
        relative.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let path_bytes = relative.to_string_lossy().as_bytes().to_vec();

    let file_type = if meta.is_dir() {
        FileType::Directory
    } else if meta.is_symlink() {
        let target = fs::read_link(path).map_err(|e| Error::io_path(path, e))?;
        #[cfg(unix)]
        let target_bytes = {
            use std::os::unix::ffi::OsStrExt;
            target.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let target_bytes = target.to_string_lossy().as_bytes().to_vec();
        FileType::Symlink(target_bytes)
    } else {
        FileType::File
    };

    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);

    #[cfg(unix)]
    let (mode, uid, gid) = (meta.mode(), meta.uid(), meta.gid());
    #[cfg(not(unix))]
    let (mode, uid, gid) = (0o644u32, 0u32, 0u32);

    Ok(FileEntry {
        path: path_bytes,
        file_type,
        mode,
        uid,
        gid,
        mtime_ns,
        size: meta.len(),
        block_refs: vec![],
        xattrs: Default::default(),
        snapshot_id: None,
    })
}

/// Restore metadata (permissions, timestamps) on an extracted file.
#[cfg(unix)]
pub fn restore_metadata(path: &Path, entry: &FileEntry) -> Result<()> {
    // Restore permissions
    let perms = fs::Permissions::from_mode(entry.mode);
    fs::set_permissions(path, perms).map_err(|e| Error::io_path(path, e))?;

    // Restore mtime
    if entry.mtime_ns > 0 {
        let duration = std::time::Duration::from_nanos(entry.mtime_ns as u64);
        let mtime = filetime::FileTime::from_unix_time(
            duration.as_secs() as i64,
            duration.subsec_nanos(),
        );
        filetime::set_file_mtime(path, mtime).map_err(|e| Error::io_path(path, e))?;
    }

    Ok(())
}

#[cfg(not(unix))]
pub fn restore_metadata(_path: &Path, _entry: &FileEntry) -> Result<()> {
    // Minimal metadata restoration on non-Unix
    Ok(())
}

/// Validate an extraction path for safety (path traversal, absolute paths).
pub fn validate_extraction_path(entry_path: &[u8], dest: &Path) -> Result<std::path::PathBuf> {
    let path_str = String::from_utf8_lossy(entry_path);

    // Reject absolute paths
    if path_str.starts_with('/') || path_str.starts_with('\\') {
        return Err(Error::PathTraversal(format!(
            "absolute path rejected: {}",
            path_str
        )));
    }

    // Reject path traversal components
    for component in path_str.split(['/', '\\']) {
        if component == ".." {
            return Err(Error::PathTraversal(format!(
                "path traversal rejected: {}",
                path_str
            )));
        }
    }

    let target = dest.join(path_str.as_ref());

    // Final check: resolved path must be under dest
    // (handles edge cases with symlinks in the path)
    if let Ok(canonical_dest) = dest.canonicalize() {
        // The target may not exist yet, so canonicalize what we can
        if let Ok(canonical_target) = target.canonicalize() {
            if !canonical_target.starts_with(&canonical_dest) {
                return Err(Error::PathTraversal(format!(
                    "resolved path escapes destination: {}",
                    path_str
                )));
            }
        }
    }

    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_absolute_path() {
        let dest = PathBuf::from("/tmp/extract");
        assert!(validate_extraction_path(b"/etc/passwd", &dest).is_err());
    }

    #[test]
    fn rejects_path_traversal() {
        let dest = PathBuf::from("/tmp/extract");
        assert!(validate_extraction_path(b"../../../etc/passwd", &dest).is_err());
        assert!(validate_extraction_path(b"foo/../../bar", &dest).is_err());
    }

    #[test]
    fn accepts_normal_path() {
        let dest = PathBuf::from("/tmp/extract");
        let result = validate_extraction_path(b"src/main.rs", &dest);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PathBuf::from("/tmp/extract/src/main.rs"));
    }

    #[test]
    fn accepts_nested_path() {
        let dest = PathBuf::from("/tmp/extract");
        let result = validate_extraction_path(b"a/b/c/d.txt", &dest);
        assert!(result.is_ok());
    }
}
