//! Port of `bundle/lib/plugins/src/types.ts` lifecycles + `cleanup`/`decorate`.
//!
//! TS stages → Rust methods:
//! | TS | Rust | APIs available |
//! |----|------|----------------|
//! | `preInit` | `pre_init` | `modules`, `patcher`, `plugins`, `react`, `assets`, `externals`, limited `discord` |
//! | `init` | `init` | + `components`, full `discord` |
//! | `start` | `start` | all APIs |
//! | `stop` | `stop` | stop-time cleanup only |

use std::collections::HashMap;
use std::sync::Arc;

use crate::discord_api::DiscordApi;
use crate::patcher::Unpatcher;
use valenced_core::manifest::PluginManifest;

/// Lifecycle stage (mirrors `PluginApiInLifecycleMap` keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lifecycle {
    Register,
    PreInit,
    Init,
    Start,
}

impl Lifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            Lifecycle::Register => "Register",
            Lifecycle::PreInit => "PreInit",
            Lifecycle::Init => "Init",
            Lifecycle::Start => "Start",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("plugin {id} error: {message}")]
    Plugin { id: String, message: String },
    #[error("missing dependency {id} (requires {range})")]
    MissingDependency { id: String, range: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Stop-time callback: `cleanup(() => ...)` in TS.
pub type PluginCleanup = Box<dyn FnOnce() + Send>;

/// Unscoped API available in `preInit` (subset of TS `UnscopedPreInitPluginApi`).
#[derive(Clone)]
pub struct UnscopedApi {
    pub discord: Arc<DiscordApi>,
}

impl UnscopedApi {
    pub fn new(discord: Arc<DiscordApi>) -> Self {
        Self { discord }
    }
}

/// Scoped plugin API instance (mirrors `Plugin` in `types.ts`).
///
/// `api` extensions registered via [`PluginContext::decorate`] are stored as
/// JSON values until a plugin reads them; WASM/JS bridges serialize through here.
pub struct PluginContext {
    pub manifest: PluginManifest,
    pub lifecycle: Lifecycle,
    pub unscoped: UnscopedApi,
    /// `decorate` extensions: `plugin_id -> key -> value`.
    pub decorations: HashMap<String, serde_json::Value>,
    cleanups: Vec<PluginCleanup>,
    unpatchers: Vec<Unpatcher>,
}

impl PluginContext {
    pub fn new(manifest: PluginManifest, lifecycle: Lifecycle, unscoped: UnscopedApi) -> Self {
        Self {
            manifest,
            lifecycle,
            unscoped,
            decorations: HashMap::new(),
            cleanups: Vec::new(),
            unpatchers: Vec::new(),
        }
    }

    /// `cleanup(fn)` — register a callback for `stop`.
    pub fn cleanup<F: FnOnce() + Send + 'static>(&mut self, f: F) {
        self.cleanups.push(Box::new(f));
    }

    /// `decorate(fn)` — extend the API for dependents.
    /// In TS the decorator mutates `plugin.api`; here it stores a JSON blob
    /// the dependent reads with [`PluginContext::extension`].
    pub fn decorate(&mut self, plugin_id: &str, key: &str, value: serde_json::Value) {
        let entry = self
            .decorations
            .entry(plugin_id.to_string())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(obj) = entry.as_object_mut() {
            obj.insert(key.to_string(), value);
        }
    }

    pub fn extension(&self, plugin_id: &str, key: &str) -> Option<serde_json::Value> {
        self.decorations.get(plugin_id)?.get(key).cloned()
    }

    #[allow(dead_code)]
    pub(crate) fn register_unpatcher(&mut self, u: Unpatcher) {
        self.unpatchers.push(u);
    }

    /// Run all `cleanup` callbacks in reverse registration order (LIFO),
    /// then unpatch everything. Mirrors the JS `stop` semantics.
    pub fn run_cleanups(&mut self) -> usize {
        let mut n = 0;
        while let Some(f) = self.cleanups.pop() {
            f();
            n += 1;
        }
        while let Some(u) = self.unpatchers.pop() {
            u.unpatch();
            n += 1;
        }
        n
    }
}

/// A Rust plugin. Implement this for native plugins; WASM plugins go through
/// [`crate::wasm_host::WasmPluginHost`]; legacy TS plugins through
/// [`crate::js_compat::JsPluginHost`].
pub trait Plugin: Send {
    fn manifest(&self) -> &PluginManifest;

    /// Runs as soon as possible, before module 0's factory. Limited APIs.
    fn pre_init(&mut self, ctx: &mut PluginContext) -> Result<(), RuntimeError> {
        let _ = ctx;
        Ok(())
    }

    /// Runs after module 0's factory, with limited APIs.
    fn init(&mut self, ctx: &mut PluginContext) -> Result<(), RuntimeError> {
        let _ = ctx;
        Ok(())
    }

    /// Runs during `AppRegistry.runApplication` with all APIs available.
    fn start(&mut self, ctx: &mut PluginContext) -> Result<(), RuntimeError> {
        let _ = ctx;
        Ok(())
    }

    /// Runs when the plugin is stopped.
    fn stop(&mut self, ctx: &mut PluginContext) -> Result<(), RuntimeError> {
        let _ = ctx;
        Ok(())
    }

    /// React-like settings UI descriptor (GPUI view or RN component name).
    /// `None` means no settings page.
    fn settings_component(&self) -> Option<String> {
        None
    }

    /// `startedLate`: true when enabled after initial startup.
    fn started_late(&self) -> bool {
        false
    }
}
