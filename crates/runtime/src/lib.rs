//! `valenced-runtime` — pure-Rust plugin runtime.
//!
//! Mirrors the TS API in `bundle/lib/plugins/src/types.ts`:
//! - Lifecycle: `preInit` → `init` → `start` → `stop` (+ `SettingsComponent`).
//! - `cleanup(…)` registers stop-time callbacks.
//! - `decorate(…)` extends the plugin API for dependents.
//!
//! New plugins are Rust crates (compiled to `cdylib` or to WASM via
//! `wasm-host`). Existing TS plugins keep working through the optional
//! [`js_compat`] bridge (Boa JS engine exposing a `valenced` global). See
//! `docs/MIGRATING_FROM_TS.md`.

pub mod discord_api;
pub mod filesystem_host;
pub mod js_compat;
pub mod patcher;
pub mod plugin;
pub mod registry;
pub mod wasm_host;

pub use discord_api::{DiscordApi, ModulesApi, ReactApi};
pub use filesystem_host::FileSystemHost;
pub use patcher::{HookKind, Patcher, Unpatcher};
pub use plugin::{Lifecycle, Plugin, PluginCleanup, PluginContext, RuntimeError, UnscopedApi};
pub use registry::Registry;
