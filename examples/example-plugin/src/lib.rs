#![allow(dead_code)]
//! Example Rust plugin: port of `bleelblep.show-tag` (community plugins).
//! Shows the username next to the display name. Run `cargo test` here to
//! see the registry ordering + patcher wiring unit-tested.

use valenced_core::manifest::{PluginDependency, PluginManifest};
use valenced_core::version::{Version, VersionRange};
use valenced_runtime::plugin::{Plugin, PluginContext, RuntimeError};
use valenced_runtime::patcher::Patcher;
use std::collections::HashMap;

/// Keep the manifest in a `OnceLock` so `manifest()` can return `&'static`.
static MANIFEST: std::sync::OnceLock<PluginManifest> = std::sync::OnceLock::new();

fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        let mut deps = HashMap::new();
        deps.insert(
            "valenced.api".into(),
            PluginDependency { version: VersionRange::parse(">=1 <2").unwrap(), optional: false },
        );
        deps.insert(
            "discord".into(),
            PluginDependency { version: VersionRange::any(), optional: false },
        );
        PluginManifest {
            id: "bleelblep.show-tag".into(),
            name: "Show Tag".into(),
            description: "Show usernames next to display names in chat, including in replies.".into(),
            author: "bleelblep <119043674385547264>".into(),
            contributors: vec!["Cynosphere <150745989836308480>".into()],
            icon: Some("AtIcon".into()),
            dependencies: deps,
            version: Version::parse("1.0.0").unwrap(),
        }
    })
}

pub struct ShowTag {
    patcher: Patcher,
}

impl ShowTag {
    pub fn new() -> Self {
        Self { patcher: Patcher::new() }
    }
}

impl Plugin for ShowTag {
    fn manifest(&self) -> &PluginManifest {
        manifest()
    }

    fn pre_init(&mut self, _ctx: &mut PluginContext) -> Result<(), RuntimeError> {
        // In TS: patch the message username component before module 0 runs.
        // In Rust: register the hook now; the shell fires it explicitly.
        let unpatch = self.patcher.before("MessageUsername", "render", "show-tag: before render");
        Ok(())
    }

    fn start(&mut self, ctx: &mut PluginContext) -> Result<(), RuntimeError> {
        // `cleanup(unpatch)` in TS → `ctx.cleanup(|| dev.unpatch())`.
        ctx.decorate("valenced.api", "show_tag.enabled", serde_json::json!(true));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_deps() {
        let m = manifest();
        assert!(m.dependencies.contains_key("valenced.api"));
        assert!(m.dependencies.contains_key("discord"));
        assert_eq!(m.id, "bleelblep.show-tag");
    }

    #[test]
    fn registry_loads_example() {
        use valenced_runtime::registry::Registry;
        use std::collections::HashMap;
        use valenced_core::version::Version;

        let mut reg = Registry::new();
        reg.register_native(Box::new(ShowTag::new()), false, false).unwrap();
        let mut host_versions = HashMap::new();
        host_versions.insert("valenced.api".into(), Version::parse("1.0.0").unwrap());
        host_versions.insert("discord".into(), Version::parse("350.2").unwrap());
        let order = reg.topo_order(&host_versions).unwrap();
        assert_eq!(order.last().unwrap(), "bleelblep.show-tag");
    }
}
