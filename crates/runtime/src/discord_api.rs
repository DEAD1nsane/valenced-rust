//! Minimal Discord API surface for Rust plugins.
//!
//! Mirrors `UnscopedPreInitPluginApi` in `bundle/lib/plugins/src/types.ts`:
//! `modules`, `patcher`, `plugins`, `react`, `assets`, `externals`,
//! `components`, `discord`. The real bundle resolves these against the live
//! Hermes runtime; the Rust runtime exposes them as typed traits with a
//! [`HostBridge`] trait that Android (JNI) / iOS (ObjC/C++) / GPUI desktop
//! implement. Unit tests use [`StubHost`].
use std::collections::HashMap;
use std::sync::Arc;

/// Host bridge implemented by each shell (Android Xposed, iOS tweak, GPUI).
pub trait HostBridge: Send + Sync {
    /// Read a Discord module by name (e.g. `main_tabs_v2`).
    fn module(&self, name: &str) -> Option<serde_json::Value>;
    /// Call a native method (`valenced.alertError`, …).
    fn call_native(
        &self,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
    /// List assets (icons, fonts) available to plugins.
    fn asset_names(&self) -> Vec<String>;
    /// Storage root for this plugin in the active slot.
    fn storage_path(&self, plugin_id: &str) -> String;
}

/// `discord` API (preInit vs init/full split is enforced by `Lifecycle`).
#[derive(Clone)]
pub struct DiscordApi {
    pub host: Arc<dyn HostBridge>,
}

impl DiscordApi {
    pub fn new(host: Arc<dyn HostBridge>) -> Self {
        Self { host }
    }

    pub fn module(&self, name: &str) -> Option<serde_json::Value> {
        self.host.module(name)
    }

    pub fn call_native(
        &self,
        method: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.host.call_native(method, args)
    }

    pub fn asset_names(&self) -> Vec<String> {
        self.host.asset_names()
    }

    /// `valenced.discord.native.FileModule.writeFile` vended through the host.
    pub fn write_file(&self, collection: &str, path: &str, contents: &str) -> Result<(), String> {
        self.host
            .call_native(
                "valenced.discord.native.FileModule.writeFile",
                serde_json::json!({ "collection": collection, "path": path, "contents": contents }),
            )
            .map(|_| ())
    }
}

/// `modules` API (Discord module lookups; debug flags via build defines).
#[derive(Clone)]
pub struct ModulesApi {
    pub discord: DiscordApi,
    pub debug_module_lookups: bool,
}

impl ModulesApi {
    pub fn get(&self, name: &str) -> Option<serde_json::Value> {
        self.discord.module(name)
    }

    pub fn require(&self, name: &str) -> Result<serde_json::Value, String> {
        self.get(name)
            .ok_or_else(|| format!("Module not found: {name}"))
    }
}

/// `react` API stub (RN `FunctionComponent`, `createElement` etc.).
/// Rust plugins return a `settings_component` id; the shell renders it.
#[derive(Clone)]
pub struct ReactApi {
    pub shell: String,
}

impl ReactApi {
    pub fn new(shell: impl Into<String>) -> Self {
        Self {
            shell: shell.into(),
        }
    }
}

/// Test / CLI host: in-memory modules + assets.
#[derive(Default)]
pub struct StubHost {
    pub modules: HashMap<String, serde_json::Value>,
    pub assets: Vec<String>,
    pub native_calls: Vec<(String, serde_json::Value)>,
}

impl HostBridge for StubHost {
    fn module(&self, name: &str) -> Option<serde_json::Value> {
        self.modules.get(name).cloned()
    }
    fn call_native(
        &self,
        _method: &str,
        _args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }
    fn asset_names(&self) -> Vec<String> {
        self.assets.clone()
    }
    fn storage_path(&self, plugin_id: &str) -> String {
        format!("/tmp/valenced/storage/{plugin_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_host() {
        let mut host = StubHost::default();
        host.modules
            .insert("main_tabs_v2".into(), serde_json::json!({ "ok": true }));
        let disc = DiscordApi::new(Arc::new(host));
        assert!(disc.module("main_tabs_v2").is_some());
        assert!(disc.module("missing").is_none());
        let mods = ModulesApi {
            discord: disc.clone(),
            debug_module_lookups: false,
        };
        assert!(mods.require("main_tabs_v2").is_ok());
        assert!(mods.require("missing").is_err());
    }
}
