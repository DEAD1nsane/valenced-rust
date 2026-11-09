//! Port of `core/.../plugins/PluginStates.kt`.
//!
//! Lifecycle flags. Flags with bits can be persisted. The JS payload names
//! must stay in sync with `bundle/lib/plugins` so the TS bundle and the Rust
//! host agree on the wire format.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::fs_util::{atomic_write_bytes, read_or_null};

/// Lifecycle flags. Only bits matter for persistence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluginFlag {
    /// The plugin is enabled.
    Enabled,
    /// The plugin is explicitly enabled by the user. Any optional
    /// disablement should not disable this plugin.
    RequiredByUser,
    /// The plugin requires a host reload to apply changes.
    PendingReload,
    /// The plugin was enabled *after* the initial load.
    StartedLate,
}

impl PluginFlag {
    pub fn bit(&self) -> u32 {
        match self {
            PluginFlag::Enabled => 1 << 0,
            PluginFlag::RequiredByUser => 1 << 1,
            PluginFlag::PendingReload => 0, // not persisted as bit in Kotlin; see persist_after_disable
            PluginFlag::StartedLate => 0,
        }
    }

    /// JS payload name (see `toJSPayload` in Kotlin).
    pub fn js_name(&self) -> &'static str {
        match self {
            PluginFlag::Enabled => "enabled",
            PluginFlag::RequiredByUser => "requiredByUser",
            PluginFlag::PendingReload => "pendingReload",
            PluginFlag::StartedLate => "startedLate",
        }
    }

    pub fn persist_after_disable(&self) -> bool {
        matches!(self, PluginFlag::PendingReload)
    }

    pub fn from_js_name(name: &str) -> Option<Self> {
        match name {
            "enabled" => Some(PluginFlag::Enabled),
            "requiredByUser" => Some(PluginFlag::RequiredByUser),
            "pendingReload" => Some(PluginFlag::PendingReload),
            "startedLate" => Some(PluginFlag::StartedLate),
            _ => None,
        }
    }
}

pub fn flags_from_bitmask(mask: u32) -> HashSet<PluginFlag> {
    let mut out = HashSet::new();
    for flag in [PluginFlag::Enabled, PluginFlag::RequiredByUser] {
        if mask & flag.bit() != 0 {
            out.insert(flag);
        }
    }
    out
}

pub fn flags_to_bitmask(flags: &HashSet<PluginFlag>) -> u32 {
    flags.iter().fold(0u32, |m, f| m | f.bit())
}

/// `{"enabled": true, ...}` as the JS bundle expects.
pub fn flags_to_js_payload(
    flags: &HashSet<PluginFlag>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    for flag in [
        PluginFlag::Enabled,
        PluginFlag::RequiredByUser,
        PluginFlag::PendingReload,
        PluginFlag::StartedLate,
    ] {
        map.insert(
            flag.js_name().to_string(),
            serde_json::Value::Bool(flags.contains(&flag)),
        );
    }
    map
}

/// Inverse of [`flags_to_js_payload`].
pub fn flags_from_js_payload(
    payload: &serde_json::Map<String, serde_json::Value>,
) -> HashSet<PluginFlag> {
    let mut out = HashSet::new();
    for name in ["enabled", "requiredByUser", "pendingReload", "startedLate"] {
        if payload.get(name) == Some(&serde_json::Value::Bool(true))
            && let Some(f) = PluginFlag::from_js_name(name) {
                out.insert(f);
            }
    }
    out
}

/// Matches `[a-zA-Z0-9._-]`, with no consecutive dots or leading dot.
/// Colons are internal-only.
pub fn is_internal_slot_id(id: &str) -> bool {
    id.starts_with(':')
}

pub fn require_valid_slot_id(id: &str) -> Result<(), String> {
    if is_internal_slot_id(id) {
        return Ok(());
    }
    let ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        && !id.contains("..")
        && !id.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(format!("Invalid slot ID: {id}"))
    }
}

const SLOT_SIDECAR_SUFFIXES: &[&str] = &["tmp", "bak", "corrupt"];

pub fn is_slot_file_name(name: &str) -> bool {
    SLOT_SIDECAR_SUFFIXES
        .iter()
        .all(|suf| !name.ends_with(&format!(".{suf}")))
}

/// On-disk representation of one slot file:
/// `{ "<plugin-id>": <bitmask> }`.
/// Kotlin stores these as ModifiedUTF-8; JSON here is equivalent for the
/// Rust host and readable by `valenced` CLI.
type SlotFile = HashMap<String, u32>;

/// A named set of plugin flags.
///
/// Persistent slots are saved as a file under `states/slots/`.
/// Ephemeral slots get discarded once the session ends.
#[derive(Debug)]
pub struct PluginStateSlot {
    pub id: String,
    /// Whether writes reach disk.
    pub persistent: bool,
    file: Option<PathBuf>,
    flags: HashMap<String, HashSet<PluginFlag>>,
}

impl PluginStateSlot {
    pub fn ephemeral(id: &str) -> Self {
        require_valid_slot_id(id).expect("slot id");
        Self {
            id: id.to_string(),
            persistent: false,
            file: None,
            flags: HashMap::new(),
        }
    }

    pub fn persistent_in(id: &str, slots_dir: &Path) -> Self {
        require_valid_slot_id(id).expect("slot id");
        let file = slots_dir.join(id);
        let initial = load_slot_file(&file);
        Self {
            id: id.to_string(),
            persistent: true,
            file: Some(file),
            flags: initial,
        }
    }

    fn load(&mut self) {
        if let Some(file) = self.file.clone() {
            self.flags = load_slot_file(&file);
        }
    }

    pub fn flags_of(&self, plugin_id: &str) -> HashSet<PluginFlag> {
        self.flags.get(plugin_id).cloned().unwrap_or_default()
    }

    /// Flags of the plugin's entry, or `None` when it has none.
    pub fn entry_flags(&self, plugin_id: &str) -> Option<HashSet<PluginFlag>> {
        self.flags.get(plugin_id).cloned()
    }

    pub fn has_plugin(&self, plugin_id: &str) -> bool {
        self.flags.contains_key(plugin_id)
    }

    pub fn is_plugin_enabled(&self, plugin_id: &str) -> bool {
        self.flags
            .get(plugin_id)
            .is_some_and(|f| f.contains(&PluginFlag::Enabled))
    }

    /// Write an entry. Returns whether anything changed.
    /// Persists to disk immediately (callers never forget to persist).
    pub fn write(&mut self, plugin_id: &str, flags: HashSet<PluginFlag>) -> bool {
        let changed = self.flags.get(plugin_id) != Some(&flags);
        if !changed {
            return false;
        }
        self.flags.insert(plugin_id.to_string(), flags);
        self.persist();
        true
    }

    pub fn set_enabled(&mut self, plugin_id: &str, enabled: bool) -> bool {
        let mut flags = self.flags_of(plugin_id);
        if enabled {
            flags.insert(PluginFlag::Enabled);
        } else {
            flags.remove(&PluginFlag::Enabled);
        }
        self.write(plugin_id, flags)
    }

    pub fn to_bitmask_map(&self) -> SlotFile {
        self.flags
            .iter()
            .map(|(k, v)| (k.clone(), flags_to_bitmask(v)))
            .collect()
    }

    /// Persist the whole slot. No-op for ephemeral slots.
    pub fn persist(&self) {
        if !self.persistent {
            return;
        }
        if let Some(file) = &self.file {
            let json = serde_json::to_string(&self.to_bitmask_map()).unwrap_or_default();
            let _ = atomic_write_bytes(file, json.as_bytes());
        }
    }

    pub fn reload_from_disk(&mut self) {
        self.load();
    }
}

fn load_slot_file(path: &Path) -> HashMap<String, HashSet<PluginFlag>> {
    let bytes = match read_or_null(path) {
        Some(b) => b,
        None => return HashMap::new(),
    };
    let raw: SlotFile = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(_) => return HashMap::new(), // corrupt file reads as empty (see Files.kt)
    };
    raw.into_iter()
        .map(|(k, mask)| (k, flags_from_bitmask(mask)))
        .collect()
}

/// Active slot pointer file (`states/active`). Empty/missing means `default`.
pub fn read_active_slot(states_dir: &Path) -> String {
    let file = states_dir.join("active");
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(_) => return "default".to_string(),
    };
    let s = String::from_utf8_lossy(&bytes).trim().to_string();
    if s.is_empty() {
        "default".to_string()
    } else {
        s
    }
}

pub fn write_active_slot(states_dir: &Path, slot_id: &str) -> std::io::Result<()> {
    crate::fs_util::atomic_write_utf8(&states_dir.join("active"), slot_id)
}

/// `var UrlEncodedRegistry` helper for tests.
#[cfg(test)]
pub fn test_slot_with(flags: HashMap<&str, HashSet<PluginFlag>>) -> PluginStateSlot {
    let mut slot = PluginStateSlot::ephemeral("default");
    for (k, v) in flags {
        slot.flags.insert(k.to_string(), v);
    }
    slot
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct JsPayload(pub serde_json::Value);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmask_roundtrip() {
        let flags: HashSet<_> = [PluginFlag::Enabled, PluginFlag::RequiredByUser]
            .into_iter()
            .collect();
        assert_eq!(flags_to_bitmask(&flags), 0b11);
        assert_eq!(flags_from_bitmask(0b11), flags);
    }

    #[test]
    fn js_payload_roundtrip() {
        let flags: HashSet<_> = [PluginFlag::Enabled].into_iter().collect();
        let payload = flags_to_js_payload(&flags);
        assert_eq!(payload["enabled"], serde_json::Value::Bool(true));
        assert_eq!(payload["pendingReload"], serde_json::Value::Bool(false));
        assert_eq!(flags_from_js_payload(&payload), flags);
    }

    #[test]
    fn slot_validation() {
        assert!(require_valid_slot_id("default").is_ok());
        assert!(require_valid_slot_id(":internal").is_ok());
        assert!(require_valid_slot_id("a..b").is_err());
        assert!(is_slot_file_name("default") && !is_slot_file_name("default.tmp"));
    }

    #[test]
    fn slot_persist_roundtrip() {
        let dir = std::env::temp_dir().join(format!("valenced-slot-{}", std::process::id()));
        let slots = dir.join("slots");
        std::fs::create_dir_all(&slots).unwrap();
        let mut slot = PluginStateSlot::persistent_in("default", &slots);
        let flags: HashSet<_> = [PluginFlag::Enabled].into_iter().collect();
        assert!(slot.write("com.example.a", flags.clone()));
        // No change → false.
        assert!(!slot.write("com.example.a", flags.clone()));
        assert!(slot.is_plugin_enabled("com.example.a"));

        let reloaded = PluginStateSlot::persistent_in("default", &slots);
        assert_eq!(reloaded.flags_of("com.example.a"), flags);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
