//! Port of `core/.../plugins/PluginPaths.kt`.

use std::path::{Path, PathBuf};

/// Every path the plugin system owns, rooted at the host app's data directory.
///
/// ```text
/// files/valenced/plugins/
///   dist/<id>/          installed plugin files
///   storage/<slot>/<id> per-slot plugin storage
///   states/             slot pointers and slot files
///   repos/              repos.json, sources.json and the index caches
/// ```
///
/// `root_override` exists for the iOS tweak, whose existing on-disk layout is
/// `<Documents>/valenced/plugins` rather than the Android-style
/// `<dataDir>/files/valenced/plugins`.
#[derive(Clone, Debug)]
pub struct PluginPaths {
    /// Host data dir (e.g. Android `filesDir`, usually `.../files`).
    pub data_dir: PathBuf,
    /// Plugin root (defaults to `data_dir / "files/valenced/plugins"`).
    pub root: PathBuf,
}

impl PluginPaths {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        let root = data_dir.join("files").join("valenced").join("plugins");
        Self { data_dir, root }
    }

    pub fn with_root_override(data_dir: impl Into<PathBuf>, root_override: impl Into<PathBuf>) -> Self {
        Self { data_dir: data_dir.into(), root: root_override.into() }
    }

    /// Installed plugin directories, one per plugin ID.
    pub fn dist(&self) -> PathBuf {
        self.root.join("dist")
    }

    /// Parent of the per-slot storage roots.
    pub fn storage(&self) -> PathBuf {
        self.root.join("storage")
    }

    pub fn states(&self) -> PathBuf {
        self.root.join("states")
    }

    pub fn slots(&self) -> PathBuf {
        self.states().join("slots")
    }

    /// Pointer file naming the slot the user chose.
    pub fn active_slot_file(&self) -> PathBuf {
        self.states().join("active")
    }

    /// Pointer file naming the one-shot slot, consumed on the next boot.
    pub fn next_slot_file(&self) -> PathBuf {
        self.states().join("next")
    }

    pub fn repos(&self) -> PathBuf {
        self.root.join("repos")
    }

    pub fn repos_json(&self) -> PathBuf {
        self.repos().join("repos.json")
    }

    pub fn sources_json(&self) -> PathBuf {
        self.repos().join("sources.json")
    }

    /// Plugin storage root for one slot, so slots isolate plugin data as
    /// well as flags.
    pub fn storage_for(&self, slot_id: &str) -> PathBuf {
        self.storage().join(slot_id)
    }

    /// A plugin's storage directory in every slot that has one.
    /// Uninstall is a cross-slot action: a directory left behind would
    /// resurrect the plugin's data on reinstall.
    pub fn storage_dirs_of_all_slots(&self, plugin_id: &str) -> Vec<PathBuf> {
        let storage = self.storage();
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&storage) {
            Ok(e) => e,
            Err(_) => return out,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.push(p.join(plugin_id));
            }
        }
        out.sort();
        out
    }

    pub fn ensure_layout(&self) -> std::io::Result<()> {
        for dir in [self.dist(), self.storage(), self.states(), self.slots(), self.repos()] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn dist_for(&self, plugin_id: &str) -> PathBuf {
        self.dist().join(plugin_id)
    }

    /// Absolute path of a file `rel` inside the data dir (loader config etc.).
    pub fn data(&self, rel: &str) -> PathBuf {
        self.data_dir.join(rel)
    }

    /// Test helper: in-memory layout under a temp dir.
    #[cfg(test)]
    pub fn for_test(base: &Path) -> Self {
        Self::new(base.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let p = PluginPaths::new("/data/files");
        assert_eq!(p.root, PathBuf::from("/data/files/files/valenced/plugins"));
        assert_eq!(p.dist(), PathBuf::from("/data/files/files/valenced/plugins/dist"));
        assert_eq!(p.storage_for("default"), PathBuf::from("/data/files/files/valenced/plugins/storage/default"));
        assert_eq!(p.repos_json(), PathBuf::from("/data/files/files/valenced/plugins/repos/repos.json"));
    }

    #[test]
    fn iOS_override() {
        let p = PluginPaths::with_root_override("/Documents", "/Documents/valenced/plugins");
        assert_eq!(p.root, PathBuf::from("/Documents/valenced/plugins"));
    }
}
