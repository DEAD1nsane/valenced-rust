//! Port of `core/.../Files.kt`: atomic writes, tolerant reads.
//!
//! The Kotlin `atomicWrite` writes a sibling `.tmp`, moves the old file to
//! `.bak`, moves `.tmp` into place, and restores `.bak` on failure. This Rust
//! version does the same so a crash never leaves a half-written store.

use std::fs;
use std::path::{Path, PathBuf};

/// Sibling path with `suffix` appended (e.g. `foo.json` + `.tmp`).
pub fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(suffix);
    path.with_file_name(name)
}

/// Write `bytes` to `target` via a sibling `.tmp` file, keeping the old
/// contents in a sibling `.bak` until the move lands.
///
/// This mirrors `FileSystem.atomicWrite` in `Files.kt`. A move that fails
/// (e.g. some Android filesystems refuse to replace an existing file)
/// restores the backup instead of leaving nothing behind.
pub fn atomic_write_bytes(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = target.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Cannot write to a filesystem root",
        )
    })?;
    if !parent.as_os_str().is_empty() {
        fs::create_dir_all(parent)?;
    }
    let tmp = sibling(target, ".tmp");
    let backup = sibling(target, ".bak");

    fs::write(&tmp, bytes)?;

    let had_target = target.exists();
    let restorable = had_target && !backup.exists();
    if restorable {
        fs::rename(target, &backup)?;
    } else if had_target {
        let _ = fs::remove_file(target);
    }

    if let Err(e) = fs::rename(&tmp, target) {
        if restorable {
            let _ = fs::rename(&backup, target);
        }
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    let _ = fs::remove_file(&backup);
    Ok(())
}

/// [`atomic_write_bytes`] for UTF-8 text — every store but the states codec.
pub fn atomic_write_utf8(target: &Path, text: &str) -> std::io::Result<()> {
    atomic_write_bytes(target, text.as_bytes())
}

/// Read `path` as text, or `None` when it is absent or unreadable.
pub fn read_utf8_or_null(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Read `path`, or `None` when it is absent, empty or unreadable.
pub fn read_or_null(path: &Path) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    fs::read(path).ok()
}

/// Names of the regular files directly under `dir`, or empty when absent.
pub fn list_file_names(dir: &Path) -> Vec<String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                out.push(name.to_string());
            }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_roundtrip() {
        let dir = std::env::temp_dir().join(format!("valenced-atomic-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let target = dir.join("store.json");
        atomic_write_utf8(&target, r#"{"a":1}"#).unwrap();
        assert_eq!(read_utf8_or_null(&target).as_deref(), Some(r#"{"a":1}"#));
        atomic_write_utf8(&target, r#"{"a":2}"#).unwrap();
        assert_eq!(read_utf8_or_null(&target).as_deref(), Some(r#"{"a":2}"#));
        // No tmp/bak left behind.
        assert!(!sibling(&target, ".tmp").exists());
        assert!(!sibling(&target, ".bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_reads_are_null() {
        assert!(read_utf8_or_null(Path::new("/nonexistent/valenced.json")).is_none());
        assert!(read_or_null(Path::new("/nonexistent/valenced.json")).is_none());
        assert!(list_file_names(Path::new("/nonexistent/dir")).is_empty());
    }
}
