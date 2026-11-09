//! Port of `core/.../plugins/repos/RepoIndex.kt` and `PluginSource.kt`.
//!
//! Sanitizing is the important part: structurally invalid versions/plugins
//! are dropped with a warning, never fatal, so a single bad entry cannot
//! brick a repository.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::manifest::{validated_plugin_icon, require_valid_plugin_id, ExternalDependency};
use crate::version::{Version, VersionRange};

/// The supported repository index format version.
pub const REPO_INDEX_FORMAT: i64 = 1;

/// The default channel used for installs and update checks.
pub const REPO_CHANNEL_LATEST: &str = "latest";

/// The URL is its identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoIndex {
    pub format: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub icon: Option<String>,
    #[serde(default)]
    pub plugins: HashMap<String, RepoPlugin>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoPlugin {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub contributors: Vec<String>,
    pub icon: Option<String>,
    /// Channel pointers (e.g. `latest`, `testing`), each naming a key of `versions`.
    #[serde(default)]
    pub channels: HashMap<String, String>,
    /// Published versions keyed by their exact version string.
    #[serde(default)]
    pub versions: HashMap<String, RepoVersion>,
}

/// A published plugin version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoVersion {
    /// Absolute URL of the plugin ZIP artifact.
    pub url: String,
    /// Lowercase hex SHA-256 digest of the artifact.
    pub sha256: String,
    /// Artifact size in bytes.
    pub size: i64,
    /// The version's dependencies as they appear in its manifest.
    /// Used for client-side resolution before download.
    #[serde(default)]
    pub dependencies: HashMap<String, ExternalDependency>,
}

fn is_absolute_http_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Returns `Ok(())` if the entry is valid, else a human-readable problem.
fn validate_repo_version(version_key: &str, version: &RepoVersion) -> Result<(), String> {
    Version::parse(version_key).map_err(|e| format!("invalid version string: {e}"))?;
    if !is_absolute_http_url(&version.url) {
        return Err(format!("url must be absolute: '{}'", version.url));
    }
    if !is_sha256_hex(&version.sha256) {
        return Err("malformed sha256 digest".into());
    }
    if version.size <= 0 {
        return Err("non-positive size".into());
    }
    for (dep_id, dep) in &version.dependencies {
        require_valid_plugin_id(dep_id)?;
        if let Some(v) = &dep.version {
            VersionRange::parse(v)?;
        }
    }
    Ok(())
}

/// Parse and sanitize a repository index document.
///
/// Throws on unparseable JSON or unsupported `format`. Sanitizes:
/// - Icons that aren't asset names or data URLs are stripped.
/// - Structurally invalid versions are dropped with a warning.
/// - Channel pointers naming a missing/dropped version are dropped.
/// - Plugins with an invalid ID or no remaining versions are dropped.
///
/// Warnings are returned as `Vec<String>` (mirrors `Logger.w` in Kotlin).
pub fn parse_repo_index(json: &str, repo_url: &str) -> Result<(RepoIndex, Vec<String>), String> {
    let mut warnings: Vec<String> = Vec::new();
    let warn = |warnings: &mut Vec<String>, msg: String| {
        warnings.push(format!("Repository '{repo_url}': {msg}"));
    };

    let raw: RepoIndex = serde_json::from_str(json).map_err(|e| format!("Unable to parse repo index JSON: {e}"))?;
    if raw.format != REPO_INDEX_FORMAT {
        return Err(format!(
            "Repository '{repo_url}' has unsupported index format {} (supported: {REPO_INDEX_FORMAT})",
            raw.format
        ));
    }

    let mut plugins: HashMap<String, RepoPlugin> = HashMap::new();
    for (id, plugin) in &raw.plugins {
        if require_valid_plugin_id(id).is_err() {
            warn(&mut warnings, format!("dropped plugin entry '{id}': invalid plugin ID"));
            continue;
        }

        let mut versions: HashMap<String, RepoVersion> = HashMap::new();
        for (version_key, version) in &plugin.versions {
            if let Err(problem) = validate_repo_version(version_key, version) {
                warn(&mut warnings, format!("dropped version '{id}@{version_key}': {problem}"));
                continue;
            }
            versions.insert(version_key.clone(), version.clone());
        }
        if versions.is_empty() {
            warn(&mut warnings, format!("dropped plugin entry '{id}': no valid versions"));
            continue;
        }

        let mut channels: HashMap<String, String> = HashMap::new();
        for (channel, target) in &plugin.channels {
            if !versions.contains_key(target) {
                warn(&mut warnings, format!("dropped channel '{id}/{channel}': it points to missing version '{target}'"));
                continue;
            }
            channels.insert(channel.clone(), target.clone());
        }

        plugins.insert(
            id.clone(),
            RepoPlugin {
                name: plugin.name.clone(),
                description: plugin.description.clone(),
                author: plugin.author.clone(),
                contributors: plugin.contributors.clone(),
                icon: plugin.icon.as_deref().and_then(validated_plugin_icon),
                channels,
                versions,
            },
        );
    }

    Ok((
        RepoIndex {
            format: raw.format,
            name: raw.name,
            description: raw.description,
            icon: raw.icon.as_deref().and_then(validated_plugin_icon),
            plugins,
        },
        warnings,
    ))
}

/// Test helper: resolve the effective version for a channel (or newest).
pub fn version_for_channel(index: &RepoIndex, plugin_id: &str, channel: &str) -> Option<(Version, RepoVersion)> {
    let plugin = index.plugins.get(plugin_id)?;
    if let Some(target) = plugin.channels.get(channel) {
        let rv = plugin.versions.get(target)?;
        return Some((Version::parse(target).ok()?, rv.clone()));
    }
    // No channel pointer, fall back to newest non-labeled, else newest.
    let mut best: Option<(Version, RepoVersion)> = None;
    for (key, rv) in &plugin.versions {
        if let Ok(v) = Version::parse(key) {
            match &best {
                None => best = Some((v, rv.clone())),
                Some((bv, _)) if v > *bv => best = Some((v, rv.clone())),
                _ => {}
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_sha() -> String {
        "a".repeat(64)
    }

    #[test]
    fn sanitizes_bad_entries() {
        let json = serde_json::json!({
            "format": 1,
            "name": "Test",
            "plugins": {
                "com.example.good": {
                    "name": "Good",
                    "versions": {
                        "1.0.0": { "url": "https://a.example/x.zip", "sha256": dummy_sha(), "size": 10 }
                    }
                },
                "com.example.bad": {
                    "name": "Bad",
                    "versions": {
                        "1.0.0": { "url": "not-a-url", "sha256": "xyz", "size": -1 }
                    }
                }
            }
        })
        .to_string();
        let (idx, warnings) = parse_repo_index(&json, "https://a.example/plugins").unwrap();
        assert!(idx.plugins.contains_key("com.example.good"));
        assert!(!idx.plugins.contains_key("com.example.bad"));
        assert!(!warnings.is_empty());
    }

    #[test]
    fn drops_dangling_channels() {
        let json = serde_json::json!({
            "format": 1,
            "name": "Test",
            "plugins": {
                "com.example.a": {
                    "name": "A",
                    "channels": { "latest": "9.9.9", "testing": "1.0.0" },
                    "versions": {
                        "1.0.0": { "url": "https://a.example/x.zip", "sha256": dummy_sha(), "size": 10 }
                    }
                }
            }
        })
        .to_string();
        let (idx, warnings) = parse_repo_index(&json, "https://a.example/plugins").unwrap();
        let p = idx.plugins.get("com.example.a").unwrap();
        assert_eq!(p.channels.len(), 1);
        assert!(p.channels.contains_key("testing"));
        assert!(warnings.iter().any(|w| w.contains("dropped channel")));
    }

    #[test]
    fn rejects_bad_format() {
        let json = r#"{"format": 999, "name": "X", "plugins": {}}"#;
        assert!(parse_repo_index(json, "https://a.example/plugins").is_err());
    }
}
