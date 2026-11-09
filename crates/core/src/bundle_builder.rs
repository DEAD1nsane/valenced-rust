//! Rust port of `bundle/scripts/manifest.ts` — internal plugin manifest scan.
//!
//! Walks `<plugins-dir>/<stage>/<plugin>/manifest.json`, validates ids
//! (unique), `enabledByDefault`, `defaultSource`, `version` rules, and
//! emits a `BundleManifest` JSON the loaders embed. Reproducible: no
//! git/env reads — callers pass `version`, `dev`, and provider URLs.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::manifest::{BundleManifest, BundlePlugin, PluginSource, require_valid_plugin_id};
use crate::version::Version;

/// Raw internal manifest shape from `bundle/scripts/manifest.ts`
/// (`RawBundleManifestPlugin`).
#[derive(Debug, Deserialize)]
struct RawManifest {
    id: String,
    #[serde(default)]
    essential: bool,
    #[serde(default)]
    #[serde(rename = "enabledByDefault")]
    enabled_by_default: Option<serde_json::Value>,
    #[serde(default)]
    dependencies: HashMap<String, RawDependency>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    author: Option<serde_json::Value>,
    #[serde(default)]
    contributors: Vec<serde_json::Value>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    #[serde(rename = "defaultSource")]
    default_source: Option<serde_json::Value>,
    #[serde(default)]
    build: Option<RawBuild>,
}

#[derive(Debug, Deserialize)]
struct RawDependency {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    optional: bool,
}

#[derive(Debug, Deserialize)]
struct RawBuild {
    #[serde(default, rename = "devOnly")]
    dev_only: bool,
}

fn contributor_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => {
            let name = o.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let id = o.get("id").and_then(|x| x.as_str()).unwrap_or("");
            if id.is_empty() {
                name.to_string()
            } else {
                format!("{name} <{id}>")
            }
        }
        _ => v.to_string(),
    }
}

fn parse_enabled_by_default(v: Option<serde_json::Value>, dev: bool) -> bool {
    match v {
        None => false,
        Some(serde_json::Value::Bool(b)) => b,
        Some(serde_json::Value::String(s)) if s == "dev" => dev,
        Some(_) => false,
    }
}

/// Scan `plugins_dir` and produce a [`BundleManifest`].
///
/// `version` is the bundle version (e.g. from `bundle/package.json`).
/// `default_repo_url` fills `default_source.repo` when a stub declares
/// `defaultSource: true` (mirrors `VALENCED_DEFAULT_PLUGIN_REPOSITORY_URL`).
pub fn generate_bundle_manifest(
    plugins_dir: &Path,
    version: &str,
    dev: bool,
    default_repo_url: Option<&str>,
) -> Result<BundleManifest, String> {
    Version::parse(version)?;
    let mut plugins: Vec<BundlePlugin> = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();

    let stages = std::fs::read_dir(plugins_dir).map_err(|e| format!("read {}: {e}", plugins_dir.display()))?;
    for stage_entry in stages.flatten() {
        let stage_path = stage_entry.path();
        if !stage_path.is_dir() {
            continue;
        }
        let stage_name = stage_path.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let plugin_dirs = std::fs::read_dir(&stage_path).map_err(|e| format!("read {}: {e}", stage_path.display()))?;
        for plugin_entry in plugin_dirs.flatten() {
            let plugin_path = plugin_entry.path();
            if !plugin_path.is_dir() {
                continue;
            }
            let manifest_path = plugin_path.join("manifest.json");
            if !manifest_path.exists() {
                continue;
            }
            let where_ = format!("{stage_name}/{}", plugin_path.file_name().and_then(|s| s.to_str()).unwrap_or("?"));
            let json = std::fs::read_to_string(&manifest_path).map_err(|e| format!("read {where_}: {e}"))?;
            let raw: RawManifest = serde_json::from_str(&json).map_err(|e| format!("parse {where_}: {e}"))?;

            if raw.build.as_ref().is_some_and(|b| b.dev_only) && !dev {
                continue;
            }
            if raw.id.is_empty() {
                return Err(format!("Internal plugin at {where_} has no ID"));
            }
            require_valid_plugin_id(&raw.id)?;
            if let Some(prev) = seen.get(&raw.id) {
                return Err(format!("Internal plugins {prev} and {where_} share the ID \"{}\"", raw.id));
            }
            seen.insert(raw.id.clone(), where_.clone());

            // Validate enabledByDefault type.
            if let Some(v) = &raw.enabled_by_default {
                let ok = matches!(v, serde_json::Value::Bool(_)) || v.as_str() == Some("dev");
                if !ok {
                    return Err(format!("Internal plugin \"{}\" has an invalid enabledByDefault: {v}", raw.id));
                }
            }
            // Validate defaultSource type (true or object).
            if let Some(v) = &raw.default_source {
                let ok = *v == serde_json::Value::Bool(true) || v.is_object();
                if !ok {
                    return Err(format!("Internal plugin \"{}\" has an invalid defaultSource: {v}", raw.id));
                }
            }
            if raw.version.is_some() && raw.default_source.is_none() {
                return Err(format!("Internal plugin \"{}\" sets a version, but only stubs (with defaultSource) can", raw.id));
            }
            if let Some(v) = &raw.version {
                if Version::parse(v).is_err() {
                    return Err(format!("Internal plugin \"{}\" has an invalid version: {v}", raw.id));
                }
            }

            let default_source = match &raw.default_source {
                None => None,
                Some(serde_json::Value::Bool(true)) => Some(PluginSource {
                    repo: default_repo_url.map(|s| s.to_string()),
                    hash: None,
                    channel: "latest".into(),
                    held: false,
                }),
                Some(serde_json::Value::Object(o)) => Some(PluginSource {
                    repo: o.get("repo").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    hash: o.get("hash").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    channel: o.get("channel").and_then(|x| x.as_str()).unwrap_or("latest").to_string(),
                    held: o.get("held").and_then(|x| x.as_bool()).unwrap_or(false),
                }),
                _ => None,
            };

            let mut deps: HashMap<String, crate::manifest::BundleDependency> = HashMap::new();
            for (dep_id, dep) in raw.dependencies {
                require_valid_plugin_id(&dep_id)?;
                if let Some(v) = &dep.version {
                    crate::version::VersionRange::parse(v)?;
                }
                deps.insert(dep_id, crate::manifest::BundleDependency { version: dep.version, optional: dep.optional });
            }

            plugins.push(BundlePlugin {
                id: raw.id.clone(),
                essential: raw.essential,
                enabled_by_default: parse_enabled_by_default(raw.enabled_by_default, dev),
                dependencies: deps,
                version: raw.version,
                name: raw.name,
                description: raw.description,
                author: raw.author.as_ref().map(|v| contributor_str(v)),
                contributors: raw.contributors.iter().map(|v| contributor_str(v)).collect(),
                icon: raw.icon,
                default_source,
            });
        }
    }

    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(BundleManifest { format: 1, version: version.to_string(), plugins })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_manifest(dir: &Path, stage: &str, plugin: &str, json: &str) {
        let p = dir.join(stage).join(plugin);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("manifest.json"), json).unwrap();
    }

    #[test]
    fn generates_from_temp_dir() {
        let base = std::env::temp_dir().join(format!("valenced-bundle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        write_manifest(&base, "preinit", "hello", r#"{"id":"com.example.hello","enabledByDefault":true,"dependencies":{"valenced.api":{}}}"#);
        write_manifest(&base, "start", "world", r#"{"id":"com.example.world","dependencies":{}}"#);
        write_manifest(&base, "preinit", "devonly", r#"{"id":"com.example.dev","build":{"devOnly":true},"dependencies":{}}"#);

        let m = generate_bundle_manifest(&base, "1.0.0", false, None).unwrap();
        assert_eq!(m.plugins.len(), 2);
        let ids: Vec<&str> = m.plugins.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"com.example.hello"));
        assert!(!ids.iter().any(|id| id.contains("dev")));

        let m_dev = generate_bundle_manifest(&base, "1.0.0", true, None).unwrap();
        assert_eq!(m_dev.plugins.len(), 3);

        // Round-trip through BundleManifest serde.
        let json = serde_json::to_string(&m).unwrap();
        let back: BundleManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.plugins.len(), 2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let base = std::env::temp_dir().join(format!("valenced-bundle-dup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        write_manifest(&base, "preinit", "a", r#"{"id":"com.example.dup","dependencies":{}}"#);
        write_manifest(&base, "start", "b", r#"{"id":"com.example.dup","dependencies":{}}"#);
        assert!(generate_bundle_manifest(&base, "1.0.0", false, None).is_err());
        let _ = fs::remove_dir_all(&base);
    }
}
