//! `FileSystemHost` — real [`HostBridge`] for CLI + GPUI desktop.
//!
//! The Android/iOS shells implement `HostBridge` via JNI/ObjC against live
//! Hermes/React Native. This host (CLI, desktop, tests) resolves modules
//! from a `modules.json` in the data dir, assets from the same file, and
//! storage via [`PluginPaths`]. `call_native` handles the one method the
//! loaders vended (`FileModule.writeFile`) and returns `null` for others
//! (mirrors the stub in `core/.../NativePayload.kt` where unknown natives
//! are no-ops in debug).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use valenced_core::paths::PluginPaths;

use crate::discord_api::HostBridge;

/// Filesystem-backed host.
#[derive(Clone)]
pub struct FileSystemHost {
    pub paths: PluginPaths,
    pub slot_id: String,
    pub modules: HashMap<String, serde_json::Value>,
    pub assets: Vec<String>,
    pub data_dir: PathBuf,
}

impl FileSystemHost {
    pub fn new(
        paths: PluginPaths,
        slot_id: &str,
        modules: HashMap<String, serde_json::Value>,
        assets: Vec<String>,
    ) -> Self {
        let data_dir = paths.data_dir.clone();
        Self {
            paths,
            slot_id: slot_id.to_string(),
            modules,
            assets,
            data_dir,
        }
    }

    /// Load `modules.json` + `assets.json` from `data_dir` when present.
    pub fn from_data_dir(paths: PluginPaths, slot_id: &str) -> Self {
        let modules_path = paths.data_dir.join("modules.json");
        let assets_path = paths.data_dir.join("assets.json");
        let modules: HashMap<String, serde_json::Value> = std::fs::read_to_string(&modules_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let assets: Vec<String> = std::fs::read_to_string(&assets_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self::new(paths, slot_id, modules, assets)
    }

    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl HostBridge for FileSystemHost {
    fn module(&self, name: &str) -> Option<serde_json::Value> {
        self.modules.get(name).cloned()
    }

    fn call_native(
        &self,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match method {
            "valenced.discord.native.FileModule.writeFile" => {
                let collection = args
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .unwrap_or("documents");
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("valenced/loader.json");
                let contents = args.get("contents").and_then(|v| v.as_str()).unwrap_or("");
                let target = self.data_dir.join(collection).join(path);
                valenced_core::fs_util::atomic_write_utf8(&target, contents)
                    .map_err(|e| format!("write {}: {e}", target.display()))?;
                Ok(serde_json::Value::Null)
            }
            _ => Ok(serde_json::Value::Null),
        }
    }

    fn asset_names(&self) -> Vec<String> {
        self.assets.clone()
    }

    fn storage_path(&self, plugin_id: &str) -> String {
        self.paths
            .storage_for(&self.slot_id)
            .join(plugin_id)
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord_api::DiscordApi;
    use crate::plugin::UnscopedApi;

    #[test]
    fn storage_path_via_paths() {
        let paths = PluginPaths::new("/tmp/data");
        let host = FileSystemHost::new(
            paths.clone(),
            "default",
            HashMap::new(),
            vec!["AtIcon".into()],
        );
        assert!(
            host.storage_path("com.example.a")
                .contains("storage/default/com.example.a")
        );
        let unscoped = UnscopedApi::new(Arc::new(DiscordApi::new(Arc::new(host))));
        assert_eq!(unscoped.discord.asset_names(), vec!["AtIcon"]);
    }

    #[test]
    fn write_file_via_native() {
        let dir = std::env::temp_dir().join(format!("valenced-fshost-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths = PluginPaths::new(dir.clone());
        paths.ensure_layout().unwrap();
        let host = FileSystemHost::new(paths.clone(), "default", HashMap::new(), vec![]);
        let disc = DiscordApi::new(Arc::new(host));
        disc.write_file("documents", "valenced/loader.json", r#"{"a":1}"#)
            .unwrap();
        let target = paths.data_dir.join("documents/valenced/loader.json");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), r#"{"a":1}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
