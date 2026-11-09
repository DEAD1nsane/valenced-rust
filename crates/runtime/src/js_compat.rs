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

use crate::discord_api::DiscordApi;
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
        let plugin_manifest = manifest
            .to_plugin_manifest()
            .map_err(|e| RuntimeError::Plugin {
                id: manifest.id.clone(),
                message: e,
            })?;
        Ok(Self {
            manifest: plugin_manifest,
            script_path: script_path.clone(),
            started_late: false,
        })
    }
}

/// In-memory JS context handle. With `js-compat`, this owns a Boa `Context`
/// with a `valenced` global; without, it only records the staged id.
#[derive(Clone)]
pub struct JsPluginHost {
    pub plugin_id: String,
    pub script_path: String,
    #[cfg(feature = "js-compat")]
    pub context: std::sync::Arc<std::sync::Mutex<boa_engine::Context>>,
}

impl std::fmt::Debug for JsPluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsPluginHost")
            .field("plugin_id", &self.plugin_id)
            .field("script_path", &self.script_path)
            .finish()
    }
}

impl JsPluginHost {
    /// Stage a plugin with a fresh Boa context (no `valenced` global).
    /// Prefer [`JsPluginHost::stage_with_api`] when a host is available.
    pub fn stage(plugin: &JsPlugin) -> Self {
        #[cfg(not(feature = "js-compat"))]
        {
            Self {
                plugin_id: plugin.manifest.id.clone(),
                script_path: plugin.script_path.clone(),
            }
        }
        #[cfg(feature = "js-compat")]
        {
            use boa_engine::Context;
            let context = Context::default();
            Self {
                plugin_id: plugin.manifest.id.clone(),
                script_path: plugin.script_path.clone(),
                context: std::sync::Arc::new(std::sync::Mutex::new(context)),
            }
        }
    }

    /// Stage with a `valenced` global wired to `discord`:
    /// ```js
    /// valenced.version       // plugin manifest version
    /// valenced.pluginId      // plugin id
    /// valenced.assets        // string[] from host
    /// ```
    /// Module lookups go through `discord.module(name)` from Rust; dynamic
    /// JS→Rust calls are staged via `DiscordApi.call_native` (see
    /// `FileSystemHost`). The `valenced.discord.modules` object is
    /// intentionally a plain value (not a live proxy) so this compiles
    /// without GC-traced closures — fill it from `DiscordApi` at stage time.
    #[cfg(feature = "js-compat")]
    pub fn stage_with_api(plugin: &JsPlugin, discord: &DiscordApi) -> Self {
        use boa_engine::{
            Context, JsString, JsValue, js_string, object::ObjectInitializer, property::Attribute,
        };

        let version = plugin.manifest.version.to_string();
        let plugin_id = plugin.manifest.id.clone();
        let assets: Vec<String> = discord.asset_names();

        let mut context = Context::default();

        // discord.modules: pre-populate from host-known modules is not
        // enumerable via HostBridge (module(name) only), so ship `{}`
        // plus the helper `hasModule` sentinel the plugin can check.
        // Callers that need live lookups should use
        // `discord.module(name)` from Rust and inject values via eval.
        let modules_json = serde_json::json!({});
        let modules_value =
            JsValue::from_json(&modules_json, &mut context).unwrap_or(JsValue::null());
        let assets_json = serde_json::json!(assets);
        let assets_value =
            JsValue::from_json(&assets_json, &mut context).unwrap_or(JsValue::null());

        let discord_obj = ObjectInitializer::new(&mut context)
            .property(js_string!("modules"), modules_value, Attribute::all())
            .property(js_string!("assets"), assets_value, Attribute::all())
            .build();

        let valenced = ObjectInitializer::new(&mut context)
            .property(
                js_string!("version"),
                JsString::from(version),
                Attribute::all(),
            )
            .property(
                js_string!("pluginId"),
                JsString::from(plugin_id.clone()),
                Attribute::all(),
            )
            .property(js_string!("discord"), discord_obj, Attribute::all())
            .build();

        let _ =
            context.register_global_property(js_string!("valenced"), valenced, Attribute::all());

        Self {
            plugin_id,
            script_path: plugin.script_path.clone(),
            context: std::sync::Arc::new(std::sync::Mutex::new(context)),
        }
    }

    #[cfg(not(feature = "js-compat"))]
    pub fn stage_with_api(plugin: &JsPlugin, _discord: &DiscordApi) -> Self {
        Self::stage(plugin)
    }

    /// Evaluate a script (e.g. the plugin `index.js` contents).
    /// Without `js-compat`, this is a no-op returning `Null`.
    #[cfg(feature = "js-compat")]
    pub fn eval(&self, script: &str) -> Result<serde_json::Value, String> {
        use boa_engine::{JsValue, Source};
        let mut ctx = self.context.lock().unwrap();
        let value = ctx
            .eval(Source::from_bytes(script))
            .map_err(|e| format!("eval: {e}"))?;
        // Best-effort JSON round-trip for primitives; objects become null.
        let json = match value {
            JsValue::Boolean(b) => serde_json::Value::Bool(b),
            JsValue::Integer(i) => serde_json::json!(i),
            JsValue::Rational(n) => serde_json::json!(n),
            JsValue::String(s) => serde_json::Value::String(s.to_std_string_lossy()),
            JsValue::Null | JsValue::Undefined => serde_json::Value::Null,
            _ => serde_json::Value::Null,
        };
        Ok(json)
    }

    #[cfg(not(feature = "js-compat"))]
    pub fn eval(&self, _script: &str) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord_api::StubHost;
    use std::sync::Arc;

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

    #[cfg(feature = "js-compat")]
    #[test]
    fn valenced_global_is_live() -> Result<(), String> {
        let manifest_json = r#"{
            "format": 1, "id": "com.example.js", "name": "JS", "version": "1.0.0",
            "dependencies": { "valenced.api": {}, "discord": {} },
            "dist": { "script": "index.js" }
        }"#;
        let ext: ExternalManifest = serde_json::from_str(manifest_json).unwrap();
        let js = JsPlugin::from_external(&ext).unwrap();
        let mut stub = StubHost::default();
        stub.assets = vec!["AtIcon".into()];
        let discord = DiscordApi::new(Arc::new(stub));
        let host = JsPluginHost::stage_with_api(&js, &discord);
        assert_eq!(host.eval("valenced.version")?.as_str(), Some("1.0.0"));
        assert_eq!(
            host.eval("valenced.pluginId")?.as_str(),
            Some("com.example.js")
        );
        Ok(())
    }
}
