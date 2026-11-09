//! Port of `bundle/lib/patcher` (CC0, ryan-0324) to Rust traits.
//!
//! The JS patcher monkey-patches functions with `before`/`after`/`instead`.
//! In Rust we don't monkey-patch (no dynamic JS), so the patcher is a
//! **hook registry**: named hooks keyed by `(target, method, kind)` that the
//! host fires explicitly. Order semantics are preserved:
//! - `before`: LIFO (last added runs first)
//! - `after`: FIFO (first added runs first)
//! - `instead`: last added wins, receives an `original` continuation

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HookKind {
    Before,
    After,
    Instead,
}

/// A registered hook. `id` is returned for later [`Unpatcher`].
#[derive(Clone, Debug)]
pub struct Hook {
    pub id: u64,
    pub target: String,
    pub method: String,
    pub kind: HookKind,
    /// Human-readable label for host logs.
    pub label: String,
}

/// Handle that unregisters a hook. Dropping it does *not* auto-unpatch;
/// call [`Unpatcher::unpatch`] explicitly (or move it into the plugin's
/// `cleanup` list so `stop` runs it).
#[derive(Debug)]
pub struct Unpatcher {
    id: u64,
    patcher: std::sync::Arc<std::sync::Mutex<PatcherInner>>,
}

impl Unpatcher {
    pub fn unpatch(&self) {
        let mut inner = self.patcher.lock().unwrap();
        inner.remove(self.id);
    }
    pub fn hook_id(&self) -> u64 {
        self.id
    }
}

#[derive(Debug, Default)]
struct PatcherInner {
    next_id: u64,
    hooks: HashMap<u64, Hook>,
}

impl PatcherInner {
    fn remove(&mut self, id: u64) -> bool {
        self.hooks.remove(&id).is_some()
    }
}

/// Hook registry. Cloneable; all clones share one registry (like the JS
/// module singleton).
#[derive(Clone, Debug, Default)]
pub struct Patcher {
    inner: std::sync::Arc<std::sync::Mutex<PatcherInner>>,
    /// Cumulative call counter for host telemetry.
    calls: Arc<AtomicU64>,
}

impl Patcher {
    pub fn new() -> Self {
        Self::default()
    }

    fn add(&self, target: &str, method: &str, kind: HookKind, label: &str) -> Unpatcher {
        let mut inner = self.inner.lock().unwrap();
        let id = inner.next_id;
        inner.next_id += 1;
        inner.hooks.insert(
            id,
            Hook {
                id,
                target: target.to_string(),
                method: method.to_string(),
                kind,
                label: label.to_string(),
            },
        );
        Unpatcher {
            id,
            patcher: self.inner.clone(),
        }
    }

    /// `before(obj, 'method', handler)` — runs before the original,
    /// may rewrite args.
    pub fn before(&self, target: &str, method: &str, label: &str) -> Unpatcher {
        self.add(target, method, HookKind::Before, label)
    }

    /// `after(obj, 'method', handler)` — runs after the original,
    /// may rewrite the return value.
    pub fn after(&self, target: &str, method: &str, label: &str) -> Unpatcher {
        self.add(target, method, HookKind::After, label)
    }

    /// `instead(obj, 'method', handler)` — fully replaces the original,
    /// receiving `original(...)` as a continuation.
    pub fn instead(&self, target: &str, method: &str, label: &str) -> Unpatcher {
        self.add(target, method, HookKind::Instead, label)
    }

    /// Hooks for one `(target, method)`, sorted in execution order:
    /// before (LIFO), instead (last wins, max 1 returned), after (FIFO).
    pub fn hooks_for(&self, target: &str, method: &str) -> (Vec<Hook>, Option<Hook>, Vec<Hook>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let inner = self.inner.lock().unwrap();
        let mut befores: Vec<_> = inner
            .hooks
            .values()
            .filter(|h| h.target == target && h.method == method && h.kind == HookKind::Before)
            .cloned()
            .collect();
        let mut afters: Vec<_> = inner
            .hooks
            .values()
            .filter(|h| h.target == target && h.method == method && h.kind == HookKind::After)
            .cloned()
            .collect();
        let instead = inner
            .hooks
            .values()
            .filter(|h| h.target == target && h.method == method && h.kind == HookKind::Instead)
            .max_by_key(|h| h.id)
            .cloned();
        befores.sort_by(|a, b| b.id.cmp(&a.id)); // LIFO
        afters.sort_by_key(|a| a.id); // FIFO
        (befores, instead, afters)
    }

    pub fn hook_count(&self) -> usize {
        self.inner.lock().unwrap().hooks.len()
    }

    pub fn total_calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().hooks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_semantics() {
        let p = Patcher::new();
        let _u1 = p.before("obj", "m", "first");
        let _u2 = p.before("obj", "m", "second");
        let _u3 = p.after("obj", "m", "first");
        let _u4 = p.after("obj", "m", "second");
        let (b, i, a) = p.hooks_for("obj", "m");
        assert_eq!(b[0].label, "second");
        assert_eq!(b[1].label, "first");
        assert_eq!(a[0].label, "first");
        assert_eq!(a[1].label, "second");
        assert!(i.is_none());
    }

    #[test]
    fn instead_last_wins_and_unpatch() {
        let p = Patcher::new();
        let _u1 = p.instead("obj", "m", "first");
        let u2 = p.instead("obj", "m", "second");
        let (_, instead, _) = p.hooks_for("obj", "m");
        assert_eq!(instead.unwrap().label, "second");
        u2.unpatch();
        let (_, instead, _) = p.hooks_for("obj", "m");
        assert_eq!(instead.unwrap().label, "first");
    }
}
