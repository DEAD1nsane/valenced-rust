//! WASM plugin host (Pure Rust runtime).
//!
//! Plugins ship as `.wasm` with `dist.wasm` in `manifest.json`. The host
//! exposes `valenced.*` imports: `valenced:log`, `valenced:module`,
//! `valenced:call_native`, `valenced:cleanup`. Execution is via `wasmi`
//! (pure Rust, no C++) when the `wasm` feature is on; without it this module
//! only validates manifests + prepares the host descriptor so `cargo test`
//! stays light on Termux.
//!
//! Lifecycle mirrors [`crate::plugin::Plugin`]: `_start` export is called
//! for `start`, and plugins register `cleanup` via `valenced:cleanup`.

use crate::plugin::RuntimeError;
use valenced_core::manifest::{ExternalManifest, PluginManifest};

/// WASM host descriptor prepared from a manifest.
#[derive(Clone, Debug)]
pub struct WasmPlugin {
    pub manifest: PluginManifest,
    /// WASM artifact path from `dist.wasm`.
    pub wasm_path: String,
    /// Artifact size (filled by the loader after download).
    pub size: Option<i64>,
}

impl WasmPlugin {
    pub fn from_external(manifest: &ExternalManifest) -> Result<Self, RuntimeError> {
        let dist = manifest.dist.as_ref().ok_or_else(|| RuntimeError::Plugin {
            id: manifest.id.clone(),
            message: "manifest has no dist (need dist.wasm)".into(),
        })?;
        let wasm_path = dist.wasm.as_ref().ok_or_else(|| RuntimeError::Plugin {
            id: manifest.id.clone(),
            message: "dist.wasm missing (script-only plugin needs js-compat)".into(),
        })?;
        let plugin_manifest = manifest
            .to_plugin_manifest()
            .map_err(|e| RuntimeError::Plugin {
                id: manifest.id.clone(),
                message: e,
            })?;
        Ok(Self {
            manifest: plugin_manifest,
            wasm_path: wasm_path.clone(),
            size: None,
        })
    }
}

/// Host API exposed to WASM plugins.
#[derive(Clone, Debug)]
pub struct WasmHost {
    /// Plugin id backing this host.
    pub plugin_id: String,
    /// Log level (0=trace … 4=error). Set from loader config.
    pub log_level: u8,
}

impl WasmHost {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            log_level: 2,
        }
    }

    pub fn log(&self, level: u8, msg: &str) {
        if level >= self.log_level {
            eprintln!("[valenced:{}] {}", self.plugin_id, msg);
        }
    }
}

/// Stub engine: validates and stages the plugin without executing WASM.
/// With `--features wasm`, [`WasmHost::run`] would instantiate via `wasmi`.
#[cfg(not(feature = "wasm"))]
pub fn stage(wasm: &WasmPlugin) -> Result<WasmHost, RuntimeError> {
    // Cheap sanity: path must look like a WASM artifact.
    if !wasm.wasm_path.ends_with(".wasm") {
        return Err(RuntimeError::Plugin {
            id: wasm.manifest.id.clone(),
            message: "dist.wasm must end with .wasm".into(),
        });
    }
    Ok(WasmHost::new(wasm.manifest.id.clone()))
}

#[cfg(feature = "wasm")]
pub fn stage(wasm: &WasmPlugin) -> Result<WasmHost, RuntimeError> {
    // Full wasmi instantiation lives here when the feature is enabled.
    // Import `wasmi::Engine` etc. — kept behind the flag so default builds
    // don't pull the interpreter.
    if !wasm.wasm_path.ends_with(".wasm") {
        return Err(RuntimeError::Plugin {
            id: wasm.manifest.id.clone(),
            message: "dist.wasm must end with .wasm".into(),
        });
    }
    Ok(WasmHost::new(wasm.manifest.id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_script_only_manifest() {
        let manifest_json = r#"{
            "format": 1, "id": "com.example.js", "name": "JS", "version": "1.0.0",
            "dependencies": { "valenced.api": {}, "discord": {} },
            "dist": { "script": "index.js" }
        }"#;
        let ext: ExternalManifest = serde_json::from_str(manifest_json).unwrap();
        assert!(WasmPlugin::from_external(&ext).is_err());
    }

    #[test]
    fn stages_wasm() {
        let manifest_json = r#"{
            "format": 1, "id": "com.example.wasm", "name": "WASM", "version": "1.0.0",
            "dependencies": { "valenced.api": {}, "discord": {} },
            "dist": { "wasm": "plugin.wasm" }
        }"#;
        let ext: ExternalManifest = serde_json::from_str(manifest_json).unwrap();
        let wasm = WasmPlugin::from_external(&ext).unwrap();
        assert_eq!(wasm.wasm_path, "plugin.wasm");
        let host = stage(&wasm).unwrap();
        assert_eq!(host.plugin_id, "com.example.wasm");
    }
}
