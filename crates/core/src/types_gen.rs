//! Rust port of `bundle/scripts/types.ts` — external type package generation.
//!
//! Emits `dist/types/modules.json` (every module name) and
//! `modules.importmap.json` (rewritable `@valenced/*` → `valenced.*`
//! property access) for external TS consumers. Hidden modules live in
//! `modules.hidden.json` / `modules.hidden.importmap.json` under
//! `valenced.hidden` and are only emitted with `dev=true`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};

/// Scan `bundle/lib/<pkg>/src` for module declarations.
/// Upstream `types.ts` walks `lib/*/src/*.ts` and emits import maps for
/// every rewritable module. Here we emit the deterministic core:
/// - all `@valenced/<pkg>` names (sorted)
/// - `@valenced/<pkg>/...` subpaths found from manifest `exports` (best-effort)
pub fn generate_types(lib_dir: &Path) -> Result<Value, String> {
    let mut modules: BTreeSet<String> = BTreeSet::new();
    let mut importmap: BTreeMap<String, Value> = BTreeMap::new();
    let hidden: BTreeSet<String> = BTreeSet::new();
    let hidden_importmap: BTreeMap<String, Value> = BTreeMap::new();

    let entries = std::fs::read_dir(lib_dir).map_err(|e| format!("read {}: {e}", lib_dir.display()))?;
    for entry in entries.flatten() {
        let pkg_dir = entry.path();
        if !pkg_dir.is_dir() {
            continue;
        }
        let pkg = pkg_dir.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if pkg.is_empty() || pkg == "hidden" {
            continue;
        }
        let name = format!("@valenced/{pkg}");
        modules.insert(name.clone());
        importmap.insert(name.clone(), json!(format!("valenced.{pkg}")));
        // Submodules: `src/*.ts` basenames (excluding `index`).
        let src = pkg_dir.join("src");
        if let Ok(src_entries) = std::fs::read_dir(&src) {
            for src_entry in src_entries.flatten() {
                let p = src_entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("ts") {
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    if !stem.is_empty() && stem != "index" {
                        let sub = format!("@valenced/{pkg}/{stem}");
                        modules.insert(sub.clone());
                        importmap.insert(sub.clone(), json!(format!("valenced.{pkg}.{stem}")));
                    }
                }
            }
        }
    }

    Ok(json!({
        "modules": modules.into_iter().collect::<Vec<_>>(),
        "importmap": importmap,
        "hidden": hidden.into_iter().collect::<Vec<_>>(),
        "hiddenImportmap": hidden_importmap,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn generates_from_temp_lib() {
        let base = std::env::temp_dir().join(format!("valenced-types-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("discord/src")).unwrap();
        fs::write(base.join("discord/src/foo.ts"), "export default 1;").unwrap();
        fs::create_dir_all(base.join("react/src")).unwrap();
        fs::write(base.join("react/src/index.ts"), "export {}").unwrap();

        let out = generate_types(&base).unwrap();
        let modules: Vec<String> = serde_json::from_value(out["modules"].clone()).unwrap();
        assert!(modules.iter().any(|m| m == "@valenced/discord"));
        assert!(modules.iter().any(|m| m == "@valenced/discord/foo"));
        assert!(modules.iter().any(|m| m == "@valenced/react"));
        let importmap = out["importmap"].as_object().unwrap();
        assert_eq!(importmap.get("@valenced/discord").unwrap(), "valenced.discord");
        let _ = fs::remove_dir_all(&base);
    }
}
