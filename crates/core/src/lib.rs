//! `valenced-core` — Rust port of the Kotlin Multiplatform core.
//!
//! Modules mirror `core/src/commonMain/kotlin/app/valenced/*`:
//! - [`version`] ← `plugins/Version.kt` (exact ordering/range semantics)
//! - [`manifest`] ← `plugins/PluginManifest.kt`, `ExternalManifest.kt`, `BundleManifest.kt`
//! - [`repos`] ← `plugins/repos/RepoIndex.kt` + `PluginSource.kt`
//! - [`resolver`] ← `plugins/repos/Resolver.kt`
//! - [`states`] ← `plugins/PluginStates.kt`
//! - [`paths`] ← `plugins/PluginPaths.kt`
//! - [`fs_util`] ← `core/Files.kt`
//! - [`theme`] ← Themes API validator (for `discord-themes`)

pub mod bundle_builder;
pub mod fs_util;
pub mod install;
pub mod manifest;
pub mod paths;
pub mod repos;
pub mod resolver;
pub mod states;
pub mod theme;
pub mod types_gen;
pub mod version;

pub use manifest::{
    API_DEPENDENCY_ID, BundleManifest, DISCORD_DEPENDENCY_ID, ExternalManifest, MANIFEST_FILE,
    PluginManifest, PluginSource,
};
pub use repos::{REPO_CHANNEL_LATEST, REPO_INDEX_FORMAT, RepoIndex};
pub use resolver::{InstallPlan, PlannedAction, ResolveRequest, resolve_install};
pub use states::{PluginFlag, PluginStateSlot};
pub use theme::{Theme, parse_theme};
pub use version::{Version, VersionRange};
