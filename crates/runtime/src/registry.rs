//! Plugin registry: dependency-ordered lifecycle driver.
//!
//! Enforces the rules from `PluginManifest.kt` / `ExternalManifest.kt`:
//! - External plugins MUST depend on `valenced.api` and `discord`.
//! - Reserved IDs resolve to host-provided versions (`valenced.api` ←
//!   [`valenced_core::manifest::API_DEPENDENCY_ID`]).
//! - Optional deps never block; required deps order first and must exist.
//! - Essential plugins cannot be disabled.
use std::collections::HashMap;

use crate::plugin::{Lifecycle, Plugin, PluginContext, RuntimeError, UnscopedApi};
use valenced_core::manifest::{PluginManifest, parse_external_manifest, require_valid_plugin_id, reserved_dependency_ids};
use valenced_core::paths::PluginPaths;

/// Registry entry: a live plugin + its manifest.
pub struct Entry {
    pub manifest: PluginManifest,
    /// Boxed plugin instance (native Rust).
    pub plugin: Option<Box<dyn Plugin>>,
    /// JS/WASM host handle (id only here; see `wasm_host` / `js_compat`).
    pub host_id: Option<String>,
    pub essential: bool,
    pub enabled_by_default: bool,
}

/// Dependency-ordered registry.
#[derive(Default)]
pub struct Registry {
    entries: HashMap<String, Entry>,
    /// Load order computed by [`Registry::topo_order`].
    order: Vec<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a native Rust plugin.
    pub fn register_native(&mut self, plugin: Box<dyn Plugin>, essential: bool, enabled_by_default: bool) -> Result<(), RuntimeError> {
        let manifest = plugin.manifest().clone();
        self.register_manifest(manifest.clone())?;
        self.entries.insert(manifest.id.clone(), Entry { manifest, plugin: Some(plugin), host_id: None, essential, enabled_by_default });
        Ok(())
    }

    /// Register a WASM/JS plugin by manifest only (logic lives in the host).
    pub fn register_external(&mut self, manifest: PluginManifest, host_id: String, essential: bool) -> Result<(), RuntimeError> {
        self.register_manifest(manifest.clone())?;
        self.entries.insert(manifest.id.clone(), Entry { manifest, plugin: None, host_id: Some(host_id), essential, enabled_by_default: false });
        Ok(())
    }

    fn register_manifest(&self, manifest: PluginManifest) -> Result<(), RuntimeError> {
        require_valid_plugin_id(&manifest.id).map_err(|e| RuntimeError::Plugin { id: manifest.id.clone(), message: e })?;
        // Reserved providers are host-side; user plugins may not reuse them.
        if reserved_dependency_ids().contains(manifest.id.as_str()) {
            return Err(RuntimeError::Plugin { id: manifest.id, message: "reserved plugin ID".into() });
        }
        Ok(())
    }

    pub fn manifest(&self, id: &str) -> Option<&PluginManifest> {
        self.entries.get(id).map(|e| &e.manifest)
    }

    pub fn is_essential(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|e| e.essential)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Topological order respecting dependencies (required first, optional
    /// when present). `host_versions` supplies `valenced.api` / `discord`.
    pub fn topo_order(&mut self, host_versions: &HashMap<String, valenced_core::Version>) -> Result<Vec<String>, RuntimeError> {
        let mut order: Vec<String> = Vec::new();
        let mut visiting: Vec<String> = Vec::new();
        let mut visited: HashMap<String, bool> = HashMap::new();

        fn visit(
            id: &str,
            registry: &Registry,
            host_versions: &HashMap<String, valenced_core::Version>,
            order: &mut Vec<String>,
            visiting: &mut Vec<String>,
            visited: &mut HashMap<String, bool>,
        ) -> Result<(), RuntimeError> {
            if visited.contains_key(id) {
                return Ok(());
            }
            if visiting.contains(&id.to_string()) {
                return Err(RuntimeError::Plugin { id: id.to_string(), message: "dependency cycle".into() });
            }
            visiting.push(id.to_string());
            if let Some(entry) = registry.entries.get(id) {
                for (dep_id, dep) in &entry.manifest.dependencies {
                    if let Some(host_ver) = host_versions.get(dep_id) {
                        if !dep.version.satisfies(host_ver) {
                            return Err(RuntimeError::MissingDependency { id: dep_id.clone(), range: dep.version.to_string() });
                        }
                        if dep.optional {
                            continue;
                        }
                        continue; // host-provided, no order needed
                    }
                    if registry.entries.contains_key(dep_id) {
                        visit(dep_id, registry, host_versions, order, visiting, visited)?;
                    } else if !dep.optional {
                        return Err(RuntimeError::MissingDependency { id: dep_id.clone(), range: dep.version.to_string() });
                    }
                }
            }
            visiting.retain(|x| x != id);
            visited.insert(id.to_string(), true);
            order.push(id.to_string());
            Ok(())
        }

        for id in self.entries.keys().cloned().collect::<Vec<_>>() {
            visit(&id, self, host_versions, &mut order, &mut visiting, &mut visited)?;
        }
        self.order = order.clone();
        Ok(order)
    }

    pub fn start_all(&mut self, unscoped: UnscopedApi, host_versions: &HashMap<String, valenced_core::Version>) -> Result<(), RuntimeError> {
        let order = self.topo_order(host_versions)?;
        for id in order {
            let entry = self.entries.get_mut(&id).ok_or_else(|| RuntimeError::Plugin { id: id.clone(), message: "not registered".into() })?;
            if let Some(plugin) = entry.plugin.as_mut() {
                for stage in [Lifecycle::PreInit, Lifecycle::Init, Lifecycle::Start] {
                    let mut ctx = PluginContext::new(entry.manifest.clone(), stage, unscoped.clone());
                    match stage {
                        Lifecycle::PreInit => plugin.pre_init(&mut ctx)?,
                        Lifecycle::Init => plugin.init(&mut ctx)?,
                        Lifecycle::Start => plugin.start(&mut ctx)?,
                        Lifecycle::Register => unreachable!(),
                    }
                }
            }
        }
        Ok(())
    }

    pub fn stop_all(&mut self, unscoped: UnscopedApi) {
        for id in self.order.clone().into_iter().rev() {
            if let Some(entry) = self.entries.get_mut(&id) {
                if let Some(plugin) = entry.plugin.as_mut() {
                    let mut ctx = PluginContext::new(entry.manifest.clone(), Lifecycle::Start, unscoped.clone());
                    let _ = plugin.stop(&mut ctx);
                    ctx.run_cleanups();
                }
            }
        }
    }

    /// Load every installed plugin under `paths.dist()` into the registry.
    /// JS/WASM plugins are registered by manifest (logic runs in their
    /// hosts); native Rust `Plugin`s are registered by the host binary
    /// via [`Registry::register_native`]. Returns `(loaded, skipped)` —
    /// skipped entries failed validation with a reason.
    pub fn load_from_dist(&mut self, paths: &PluginPaths) -> (Vec<String>, Vec<(String, String)>) {
        let dist = paths.dist();
        let mut loaded = Vec::new();
        let mut skipped = Vec::new();
        let entries = match std::fs::read_dir(&dist) {
            Ok(e) => e,
            Err(_) => return (loaded, skipped),
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let dir_name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string();
            let manifest_path = dir.join(valenced_core::manifest::MANIFEST_FILE);
            let json = match std::fs::read_to_string(&manifest_path) {
                Ok(j) => j,
                Err(e) => {
                    skipped.push((dir_name.clone(), format!("read manifest: {e}")));
                    continue;
                }
            };
            let ext = match parse_external_manifest(&json) {
                Ok(m) => m,
                Err(e) => {
                    skipped.push((dir_name.clone(), e));
                    continue;
                }
            };
            let pm = match ext.to_plugin_manifest() {
                Ok(p) => p,
                Err(e) => {
                    skipped.push((dir_name.clone(), e));
                    continue;
                }
            };
            // Host id records the dist kind for WASM/JS hosts.
            let host_id = if ext.dist.as_ref().is_some_and(|d| d.wasm.is_some()) {
                format!("wasm:{}", pm.id)
            } else if ext.dist.as_ref().is_some_and(|d| d.script.is_some()) {
                format!("js:{}", pm.id)
            } else {
                format!("native:{}", pm.id)
            };
            if let Err(e) = self.register_external(pm, host_id, false) {
                skipped.push((dir_name.clone(), e.to_string()));
                continue;
            }
            let id = self.manifest(&dir_name).map(|m| m.id.clone()).unwrap_or_else(|| dir_name.clone());
            loaded.push(id);
        }
        loaded.sort();
        (loaded, skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use valenced_core::version::{Version, VersionRange};
    use valenced_core::manifest::PluginDependency;

    #[test]
    fn registry_load_order_and_missing_required() {
        let mut reg = Registry::new();
        // Provide manifests without plugins via register_external.
        let api = PluginManifest {
            id: "dummy.provider".into(), name: "P".into(), description: String::new(),
            author: String::new(), contributors: vec![], icon: None,
            dependencies: HashMap::new(), version: Version::parse("1.0.0").unwrap(),
        };
        reg.register_external(api, "host:provider".into(), true).unwrap();
        let mut m = PluginManifest {
            id: "com.example.a".into(), name: "A".into(), description: String::new(),
            author: String::new(), contributors: vec![], icon: None,
            dependencies: HashMap::new(), version: Version::parse("1.0.0").unwrap(),
        };
        m.dependencies.insert("dummy.provider".into(), PluginDependency { version: VersionRange::any(), optional: false });
        reg.register_external(m, "host:a".into(), false).unwrap();

        let mut host_versions = HashMap::new();
        host_versions.insert("valenced.api".into(), Version::parse("1.0.0").unwrap());
        let order = reg.topo_order(&host_versions).unwrap();
        assert_eq!(order.last().unwrap(), "com.example.a");

        // Missing required fails.
        let mut reg2 = Registry::new();
        let mut m2 = PluginManifest {
            id: "com.example.b".into(), name: "B".into(), description: String::new(),
            author: String::new(), contributors: vec![], icon: None,
            dependencies: HashMap::new(), version: Version::parse("1.0.0").unwrap(),
        };
        m2.dependencies.insert("com.example.missing".into(), PluginDependency { version: VersionRange::any(), optional: false });
        reg2.register_external(m2, "host:b".into(), false).unwrap();
        assert!(reg2.topo_order(&host_versions).is_err());
    }
}
