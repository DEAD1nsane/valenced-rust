//! Port of `core/.../plugins/PluginManifest.kt`,
//! `core/.../plugins/external/ExternalManifest.kt` and
//! `core/.../plugins/BundleManifest.kt`.
//!
//! Kotlin uses `kotlinx.serialization` with `ignoreUnknownKeys = true` so old
//! manifests keep parsing. Serde does the same by default (unknown fields are
//! ignored unless `deny_unknown_fields` is set).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::version::{Version, VersionRange};

/// The current Valenced plugin API version.
///
/// Upstream defines this via `API_VERSION_STRING` in generated code.
/// Keep in sync with `bundle/package.json` `version` and with the
/// `valenced.api` provider plugin in `bundle/src/plugins/preinit/api`.
/// Until the TS bundle and Rust core share a build script, this const is
/// verified by `cargo test` against `bundle/package.json` (see `cli`).
pub const API_VERSION_STRING: &str = "1.0.0";

/// Reserved dependency ID resolving to the plugin API version.
/// External plugins **MUST** declare a dependency on this ID.
pub const API_DEPENDENCY_ID: &str = "valenced.api";

/// Reserved dependency ID resolving to the host Discord app's version.
/// The version is determined at runtime based on the app the module is
/// loaded into, so it lives in the loader, not in core. This crate only
/// defines the contract.
pub const DISCORD_DEPENDENCY_ID: &str = "discord";

/// Reserved dependency IDs, served by internal provider plugins.
pub fn reserved_dependency_ids() -> std::collections::HashSet<&'static str> {
    [API_DEPENDENCY_ID, DISCORD_DEPENDENCY_ID]
        .into_iter()
        .collect()
}

fn api_version() -> Version {
    Version::parse(API_VERSION_STRING).expect("API_VERSION_STRING must be valid")
}

/// Plugin metadata. Parsed from `manifest.json` for external plugins,
/// written in code for internal ones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub contributors: Vec<String>,
    pub icon: Option<String>,
    #[serde(default)]
    pub dependencies: HashMap<String, PluginDependency>,
    pub version: Version,
}

/// A plugin dependency specification. The dependency's plugin ID is the key
/// in `PluginManifest.dependencies`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginDependency {
    #[serde(default)]
    pub version: VersionRange,
    #[serde(default)]
    pub optional: bool,
}

/// `manifest.json` format version. Only 1 is accepted.
pub const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_FORMAT: i64 = 1;

/// External (on-disk) manifest, as published in plugin ZIPs and repo indexes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalManifest {
    pub format: i64,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub contributors: Vec<String>,
    pub icon: Option<String>,
    /// The plugin's version, e.g. `"1.0.0"`.
    pub version: String,
    #[serde(default)]
    pub dependencies: HashMap<String, ExternalDependency>,
    pub dist: Option<ExternalDist>,
}

/// `{ "<id>": { "version"?: "<range>", "optional"?: true } }`.
/// `null` version means `"*"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDependency {
    pub version: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDist {
    pub script: Option<String>,
    pub android: Option<ExternalAndroidDist>,
    /// Rust/WASM distribution (Pure Rust runtime). Exactly one of
    /// `script` / `wasm` may be set; see `docs/MIGRATING_FROM_TS.md`.
    pub wasm: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalAndroidDist {
    pub path: String,
    #[serde(rename = "class")]
    pub class_name: String,
}

/// Longest accepted `data:` icon value (128 KiB).
pub const MAX_ICON_DATA_URL_LENGTH: usize = 128 * 1024;

fn has_uri_scheme(s: &str) -> bool {
    // Mirrors `Regex("^[A-Za-z][A-Za-z0-9+.-]*:")`.
    let colon = s.find(':');
    match colon {
        Some(i) if i > 0 => {
            let scheme = &s[..i];
            scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
        }
        _ => false,
    }
}

/// Icons are either a packaged asset name or a size-capped `data:` URL.
/// Anything else is stripped. Icons are never fetched over the network.
pub fn validated_plugin_icon(icon: &str) -> Option<String> {
    if let Some(rest) = icon.strip_prefix("data:") {
        let _ = rest;
        if icon.len() <= MAX_ICON_DATA_URL_LENGTH {
            return Some(icon.to_string());
        }
        return None;
    }
    if has_uri_scheme(icon) {
        return None;
    }
    Some(icon.to_string())
}

/// Matches `[a-zA-Z0-9.-]`, with no consecutive dots or leading dot.
pub fn require_valid_plugin_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !id.contains("..")
        && !id.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(format!("Invalid plugin ID: {id}"))
    }
}

impl ExternalManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != MANIFEST_FORMAT {
            return Err(format!(
                "Plugin '{}' has unsupported manifest format {} (supported: {MANIFEST_FORMAT})",
                self.id, self.format
            ));
        }
        require_valid_plugin_id(&self.id)?;
        Version::parse(&self.version)?;
        let reserved = reserved_dependency_ids();
        for reserved_id in &reserved {
            if !self.dependencies.contains_key(*reserved_id) {
                return Err(format!(
                    "Plugin '{}' does not declare the mandatory '{reserved_id}' dependency",
                    self.id
                ));
            }
        }
        for (dep_id, dep) in &self.dependencies {
            require_valid_plugin_id(dep_id)?;
            if let Some(v) = &dep.version {
                VersionRange::parse(v)?;
            }
            if dep.optional && reserved.contains(dep_id.as_str()) {
                return Err(format!(
                    "Plugin '{id}': the '{dep_id}' dependency cannot be optional",
                    id = self.id
                ));
            }
        }
        // Rust runtime: exactly one dist target.
        if let Some(dist) = &self.dist {
            if dist.script.is_some() && dist.wasm.is_some() {
                return Err(format!(
                    "Plugin '{}' declares both dist.script and dist.wasm",
                    self.id
                ));
            }
        }
        Ok(())
    }

    pub fn to_plugin_manifest(&self) -> Result<PluginManifest, String> {
        require_valid_plugin_id(&self.id)?;
        let version = Version::parse(&self.version)?;
        let mut dependencies = HashMap::new();
        for (dep_id, dep) in &self.dependencies {
            require_valid_plugin_id(dep_id)?;
            let range = if let Some(v) = &dep.version {
                VersionRange::parse(v)?
            } else {
                VersionRange::any()
            };
            dependencies.insert(
                dep_id.clone(),
                PluginDependency {
                    version: range,
                    optional: dep.optional,
                },
            );
        }
        Ok(PluginManifest {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            author: self.author.clone(),
            contributors: self.contributors.clone(),
            icon: self.icon.as_deref().and_then(validated_plugin_icon),
            dependencies,
            version,
        })
    }
}

/// Parse + validate external manifest JSON (tolerant of unknown keys).
pub fn parse_external_manifest(json: &str) -> Result<ExternalManifest, String> {
    let m: ExternalManifest =
        serde_json::from_str(json).map_err(|e| format!("Unable to parse manifest JSON: {e}"))?;
    m.validate()?;
    Ok(m)
}

/// Resolve the `valenced.api` version for dependency checks.
pub fn api_version_for_check() -> Version {
    api_version()
}

/// Bundled-plugin manifest embedded in loaders. See `BundleManifest.kt`.
pub const BUNDLE_MANIFEST_FORMAT: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleManifest {
    #[serde(default = "default_bundle_format")]
    pub format: i64,
    pub version: String,
    #[serde(default)]
    pub plugins: Vec<BundlePlugin>,
}

fn default_bundle_format() -> i64 {
    BUNDLE_MANIFEST_FORMAT
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundlePlugin {
    pub id: String,
    #[serde(default)]
    pub essential: bool,
    #[serde(default)]
    #[serde(rename = "enabledByDefault")]
    pub enabled_by_default: bool,
    #[serde(default)]
    pub dependencies: HashMap<String, BundleDependency>,
    /// Overrides the bundle version. Stubs only.
    pub version: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub contributors: Vec<String>,
    pub icon: Option<String>,
    #[serde(rename = "defaultSource")]
    pub default_source: Option<PluginSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleDependency {
    pub version: Option<String>,
    #[serde(default)]
    pub optional: bool,
}

/// Where an installed plugin came from, and how it should be updated.
/// `repo == null`: sideloaded. `repo != null`: installed from that repo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginSource {
    pub repo: Option<String>,
    pub hash: Option<String>,
    #[serde(default = "default_channel")]
    pub channel: String,
    #[serde(default)]
    pub held: bool,
}

fn default_channel() -> String {
    crate::repos::REPO_CHANNEL_LATEST.to_string()
}

impl BundleManifest {
    /// Convert to `(entry, manifest)` pairs, skipping invalid entries
    /// (mirrors `pluginManifests()` + `pluginLog.e` on failure).
    pub fn plugin_manifests(&self) -> Vec<(BundlePlugin, PluginManifest)> {
        let bundle_version = match Version::parse(&self.version) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for entry in &self.plugins {
            if let Ok(m) = entry.to_plugin_manifest(&bundle_version) {
                out.push((entry.clone(), m));
            }
        }
        out
    }
}

impl BundlePlugin {
    pub fn to_plugin_manifest(&self, bundle_version: &Version) -> Result<PluginManifest, String> {
        require_valid_plugin_id(&self.id)?;
        if self.version.is_some() && self.default_source.is_none() {
            return Err("Only stubs can override the bundle version".into());
        }
        let mut dependencies = HashMap::new();
        for (dep_id, dep) in &self.dependencies {
            require_valid_plugin_id(dep_id)?;
            let range = if let Some(v) = &dep.version {
                VersionRange::parse(v)?
            } else {
                VersionRange::any()
            };
            dependencies.insert(
                dep_id.clone(),
                PluginDependency {
                    version: range,
                    optional: dep.optional,
                },
            );
        }
        let version = if let Some(v) = &self.version {
            Version::parse(v)?
        } else {
            bundle_version.clone()
        };
        Ok(PluginManifest {
            id: self.id.clone(),
            name: self.name.clone().unwrap_or_else(|| self.id.clone()),
            description: self.description.clone().unwrap_or_default(),
            author: self.author.clone().unwrap_or_default(),
            contributors: self.contributors.clone(),
            icon: self.icon.as_deref().and_then(validated_plugin_icon),
            dependencies,
            version,
        })
    }
}

pub fn parse_bundle_manifest(json: &str) -> Option<BundleManifest> {
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_validate_ok() {
        let json = r#"{
            "format": 1,
            "id": "com.example.hello",
            "name": "Hello",
            "version": "1.0.0",
            "dependencies": {
                "valenced.api": { "version": ">=1 <2" },
                "discord": {}
            },
            "dist": { "script": "index.js" }
        }"#;
        let m = parse_external_manifest(json).unwrap();
        assert_eq!(m.id, "com.example.hello");
        let pm = m.to_plugin_manifest().unwrap();
        assert_eq!(pm.dependencies.len(), 2);
    }

    #[test]
    fn external_requires_reserved() {
        let json = r#"{
            "format": 1,
            "id": "com.example.hello",
            "name": "Hello",
            "version": "1.0.0",
            "dependencies": { "valenced.api": {} }
        }"#;
        assert!(parse_external_manifest(json).is_err());
    }

    #[test]
    fn plugin_id_rules() {
        assert!(require_valid_plugin_id("com.example.a-1").is_ok());
        assert!(require_valid_plugin_id("..a").is_err());
        assert!(require_valid_plugin_id(".a").is_err());
        assert!(require_valid_plugin_id("a..b").is_err());
        assert!(require_valid_plugin_id("a/b").is_err());
    }

    #[test]
    fn icon_validation() {
        assert_eq!(
            validated_plugin_icon("PaintPaletteIcon"),
            Some("PaintPaletteIcon".into())
        );
        assert_eq!(validated_plugin_icon("https://evil.example/x.png"), None);
        assert!(
            validated_plugin_icon(&format!("data:image/png;base64,{}", "A".repeat(200_000)))
                .is_none()
        );
    }

    #[test]
    fn wasm_and_script_mutually_exclusive() {
        let json = r#"{
            "format": 1,
            "id": "com.example.both",
            "name": "Both",
            "version": "1.0.0",
            "dependencies": { "valenced.api": {}, "discord": {} },
            "dist": { "script": "index.js", "wasm": "plugin.wasm" }
        }"#;
        assert!(parse_external_manifest(json).is_err());
    }
}
