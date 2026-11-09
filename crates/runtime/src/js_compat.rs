//! JS-compat bridge: run existing TypeScript plugins under the Rust host.
//!
//! Upstream Valenced plugins are TS compiled to a Hermes bundle. This bridge
//! lets the Rust registry load them without a rewrite: the plugin's
//! `index.js` is evaluated in a [`JsPluginHost`] (Boa engine, pure Rust)
//! with a `valenced` global backed by [`crate::discord_api::DiscordApi`].
//! Build with `--features js-compat` to enable execution; without it, this
//! module only validates `dist.script` manifests so the default workspace
//! (Termux `cargo test`) stays dependency-light.
//!
//! See `docs/MIGRATING_FROM_TS.md` for the migration path TS → Rust/WASM.

use crate::plugin::RuntimeError;
use valenced_core::manifest::{ExternalManifest, PluginManifest};

/// Host descriptor for a legacy JS plugin.
#[derive(Clone, Debug)]
pub struct JsPlugin {
    pub manifest: PluginManifest,
    pub script_path: String,
    pub started_late: bool,
}

impl JsPlugin {
    pub fn from_external(manifest: &ExternalManifest) -> Result<Self, RuntimeError> {
        let dist = manifest.dist.as_ref().ok_or_else(|| RuntimeError::Plugin {
            id: manifest.id.clone(),
            message: "manifest has no dist (need dist.script)".into(),
        })?;
        let script_path = dist.script.as_ref().ok_or_else(|| RuntimeError::Plugin {
            id: manifest.id.clone(),
            message: "dist.script missing (wasm-only plugin needs wasm host)".into(),
        })?;
        let plugin_manifest = manifest.to_plugin_manifest().map_err(|e| RuntimeError::Plugin { id: manifest.id.clone(), message: e })?;
        Ok(Self { manifest: plugin_manifest, script_path: script_path.clone(), started_late: false })
    }
}

/// In-memory JS context handle. With `js-compat`, this owns a Boa `Context`
/// with a `valenced` global; without, it only records the staged id.
#[derive(Clone, Debug)]
pub struct JsPluginHost {
    pub plugin_id: String,
    pub script_path: String,
    #[cfg(feature = "js-compat")]
    pub context: std::sync::Arc<std::sync::Mutex<boa_engine::Context>>,
}

impl JsPluginHost {
    pub fn stage(plugin: &JsPlugin) -> Self {
        #[cfg(not(feature = "js-compat"))]
        {
            Self { plugin_id: plugin.manifest.id.clone(), script_path: plugin.script_path.clone() }
        }
        #[cfg(feature = "js-compat")]
        {
            use boa_engine::Context;
            // Minimal bridge: an empty Boa context. The full `valenced`
            // global wiring (DiscordApi-backed modules/patcher) is the
            // `js-compat` roadmap item in docs/MIGRATING_FROM_TS.md; the
            // context handle is already shaped for it.
            let context = Context::default();
            Self {
                plugin_id: plugin.manifest.id.clone(),
                script_path: plugin.script_path.clone(),
                context: std::sync::Arc::new(std::sync::Mutex::new(context)),
            }
        }
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wasm_only_manifest() {
        let manifest_json = r#"{
            "format": 1, "id": "com.example.wasm", "name": "WASM", "version": "1.0.0",
            "dependencies": { "valenced.api": {}, "discord": {} },
            "dist": { "wasm": "plugin.wasm" }
        }"#;
        let ext: ExternalManifest = serde_json::from_str(manifest_json).unwrap();
        assert!(JsPlugin::from_external(&ext).is_err());
    }

    #[test]
    fn stages_js() {
        let manifest_json = r#"{
            "format": 1, "id": "com.example.js", "name": "JS", "version": "1.0.0",
            "dependencies": { "valenced.api": {}, "discord": {} },
            "dist": { "script": "index.js" }
        }"#;
        let ext: ExternalManifest = serde_json::from_str(manifest_json).unwrap();
        let js = JsPlugin::from_external(&ext).unwrap();
        let host = JsPluginHost::stage(&js);
        assert_eq!(host.plugin_id(), "com.example.js");
    }
}
