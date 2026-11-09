//! Repo install execution: plan → fetch → verify → extract → record.
//!
//! Mirrors the install leg of `core/.../plugins/repos/RepoInstall.kt`
//! (the Kotlin side downloads the artifact, checks `sha256`/`size`, extracts
//! it under `dist/<id>/`, then records `PluginSource` in `sources.json`).
//!
//! Network is abstracted behind [`Fetcher`] so tests use in-memory bytes
//! while the CLI uses `ureq` for real `https://` URLs.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifest::PluginSource;
use crate::paths::PluginPaths;
use crate::resolver::{InstallPlan, PlannedAction};

/// Fetches an artifact by absolute URL.
pub trait Fetcher: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// `ureq` fetcher (rustls) for the CLI. Supports `http://`/`https://`
/// and plain `file://` (via `file:///` prefix) for tests/offline use.
#[derive(Debug, Default)]
pub struct UreqFetcher;

impl Fetcher for UreqFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        if let Some(path) = url.strip_prefix("file://") {
            return std::fs::read(path).map_err(|e| format!("read {path}: {e}"));
        }
        let resp = ureq::get(url).call().map_err(|e| format!("fetch {url}: {e}"))?;
        let mut bytes = Vec::new();
        resp.into_reader()
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read {url}: {e}"))?;
        Ok(bytes)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("fetch {url}: {msg}")]
    Fetch { url: String, msg: String },
    #[error("size mismatch for {id}: expected {expected}, got {actual}")]
    Size { id: String, expected: i64, actual: usize },
    #[error("sha256 mismatch for {id}: expected {expected}, got {actual}")]
    Checksum { id: String, expected: String, actual: String },
    #[error("extract {id}: {msg}")]
    Extract { id: String, msg: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("plan: {0}")]
    Plan(String),
}

/// Verify size + sha256 of a fetched artifact.
pub fn verify_artifact(id: &str, bytes: &[u8], expected_sha: &str, expected_size: i64) -> Result<(), InstallError> {
    if expected_size >= 0 && bytes.len() as i64 != expected_size {
        return Err(InstallError::Size { id: id.into(), expected: expected_size, actual: bytes.len() });
    }
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = format!("{:x}", hasher.finalize());
    if digest != expected_sha.to_lowercase() {
        return Err(InstallError::Checksum { id: id.into(), expected: expected_sha.into(), actual: digest });
    }
    Ok(())
}

/// Extract a ZIP artifact into `dest`, refusing `..` escapes.
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), String> {
    use std::io::Cursor;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("open zip: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("entry {i}: {e}"))?;
        let out_path = file.enclosed_name().ok_or_else(|| format!("entry {i}: unsafe path"))?;
        let out = dest.join(out_path);
        if file.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("mkdir {}: {e}", out.display()))?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
            }
            let mut out_file = std::fs::File::create(&out).map_err(|e| format!("create {}: {e}", out.display()))?;
            std::io::copy(&mut file, &mut out_file).map_err(|e| format!("write {}: {e}", out.display()))?;
            // Preserve executable bit for scripts.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(perm) = file.unix_mode().map(|m| m & 0o777) {
                    let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(perm));
                }
            }
        }
    }
    Ok(())
}

/// Load `sources.json` (`id -> PluginSource`). Missing/corrupt → empty.
pub fn load_sources(paths: &PluginPaths) -> HashMap<String, PluginSource> {
    let bytes = match crate::fs_util::read_or_null(&paths.sources_json()) {
        Some(b) => b,
        None => return HashMap::new(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Persist `sources.json` atomically.
pub fn save_sources(paths: &PluginPaths, sources: &HashMap<String, PluginSource>) -> Result<(), InstallError> {
    let json = serde_json::to_string_pretty(sources).map_err(|e| InstallError::Plan(e.to_string()))?;
    crate::fs_util::atomic_write_utf8(&paths.sources_json(), &json)?;
    Ok(())
}

fn extract_action(action: &PlannedAction, bytes: &[u8], paths: &PluginPaths) -> Result<(), InstallError> {
    let dest = paths.dist_for(&action.id);
    // Empty previous install first so stale files don't linger.
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }
    std::fs::create_dir_all(&dest)?;
    extract_zip(bytes, &dest).map_err(|e| InstallError::Extract { id: action.id.clone(), msg: e })?;
    Ok(())
}

/// Execute an [`InstallPlan`]: fetch, verify, extract, record provenance.
///
/// Returns the installed plugin ids in plan order. Empty plan (already
/// up-to-date) returns an empty vec.
pub fn install_plan<F: Fetcher>(
    plan: &InstallPlan,
    fetcher: &F,
    paths: &PluginPaths,
) -> Result<Vec<String>, InstallError> {
    paths.ensure_layout()?;
    let mut sources = load_sources(paths);
    let mut installed = Vec::new();

    for action in &plan.actions {
        let bytes = fetcher.fetch(&action.url).map_err(|e| InstallError::Fetch { url: action.url.clone(), msg: e })?;
        verify_artifact(&action.id, &bytes, &action.sha256, action.size)?;
        extract_action(action, &bytes, paths)?;

        let held = if let Some(hold) = action.hold {
            hold
        } else {
            sources.get(&action.id).map(|s| s.held).unwrap_or(false)
        };
        sources.insert(
            action.id.clone(),
            PluginSource {
                repo: Some(action.repo.clone()),
                hash: Some(action.sha256.clone()),
                channel: action.channel.clone(),
                held,
            },
        );
        installed.push(action.id.clone());
    }

    save_sources(paths, &sources)?;
    Ok(installed)
}

/// Convenience for the CLI: install one root id from one index.
pub fn install_one<F: Fetcher>(
    id: &str,
    repo_url: &str,
    index: &crate::repos::RepoIndex,
    installed: &HashMap<String, crate::version::Version>,
    options: crate::resolver::ResolveRequest,
    fetcher: &F,
    paths: &PluginPaths,
) -> Result<InstallPlan, String> {
    let sources = load_sources(paths);
    let repos = vec![(repo_url.to_string(), index.clone())];
    let plan = crate::resolver::resolve_install(&options, &repos, installed, &sources)
        .map_err(|e| e.0.to_string())?;
    install_plan(&plan, fetcher, paths).map_err(|e| e.to_string())?;
    let _ = id;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::Version;
    use std::io::Write as IoWrite;

    struct MapFetcher(HashMap<String, Vec<u8>>);
    impl Fetcher for MapFetcher {
        fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
            self.0.get(url).cloned().ok_or_else(|| format!("not found: {url}"))
        }
    }

    fn make_zip(entries: &[(&str, &[u8])]) -> (Vec<u8>, String) {
        let mut bytes = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
            let opts = zip::write::SimpleFileOptions::default();
            for (name, data) in entries {
                writer.start_file(*name, opts).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha = format!("{:x}", hasher.finalize());
        (bytes, sha)
    }

    #[test]
    fn verify_rejects_bad_hash() {
        assert!(verify_artifact("x", b"hello", &"0".repeat(64), 5).is_err());
        let (_, sha) = make_zip(&[("manifest.json", b"{}")]);
        let (bytes, _) = make_zip(&[("manifest.json", b"{}")]);
        assert!(verify_artifact("x", &bytes, &sha, bytes.len() as i64).is_ok());
    }

    #[test]
    fn install_extracts_and_records_sources() {
        let dir = std::env::temp_dir().join(format!("valenced-install-test-{}", std::process::id()));
        let paths = PluginPaths::new(dir.clone());
        paths.ensure_layout().unwrap();

        let (zip_bytes, sha) = make_zip(&[("manifest.json", br#"{"id":"com.example.a"}"#), ("plugin.wasm", b"wasm-bytes")]);
        let url = "https://a.example/a.zip".to_string();
        let mut map = HashMap::new();
        map.insert(url.clone(), zip_bytes);
        let fetcher = MapFetcher(map);

        let action = PlannedAction {
            id: "com.example.a".into(),
            version: Version::parse("1.0.0").unwrap(),
            url: url.clone(),
            sha256: sha.clone(),
            size: 0, // size checked loosely in this test (0 disables? — use real)
            repo: "https://a.example/plugins".into(),
            channel: "latest".into(),
            hold: None,
            replaces: None,
            dependents: HashMap::new(),
            candidates: HashMap::new(),
            channel_version: None,
        };
        // Fix size to real length.
        let action = PlannedAction { size: 0, ..action };
        // Recompute with real size: patch by re-fetching len.
        let real_len = fetcher.fetch(&url).unwrap().len() as i64;
        let action = PlannedAction { size: real_len, ..action };

        let plan = InstallPlan { actions: vec![action], warnings: vec![] };
        let installed = install_plan(&plan, &fetcher, &paths).unwrap();
        assert_eq!(installed, vec!["com.example.a"]);
        assert!(paths.dist_for("com.example.a").join("manifest.json").exists());
        assert!(paths.dist_for("com.example.a").join("plugin.wasm").exists());
        let sources = load_sources(&paths);
        assert_eq!(sources["com.example.a"].channel, "latest");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn size_mismatch_fails() {
        let dir = std::env::temp_dir().join(format!("valenced-install-size-{}", std::process::id()));
        let paths = PluginPaths::new(dir.clone());
        paths.ensure_layout().unwrap();
        let (zip_bytes, sha) = make_zip(&[("a.txt", b"hi")]);
        let url = "https://a.example/a.zip".to_string();
        let mut map = HashMap::new();
        map.insert(url.clone(), zip_bytes);
        let fetcher = MapFetcher(map);
        let action = PlannedAction {
            id: "x".into(),
            version: Version::parse("1.0.0").unwrap(),
            url,
            sha256: sha,
            size: 9999,
            repo: "https://a.example/plugins".into(),
            channel: "latest".into(),
            hold: None,
            replaces: None,
            dependents: HashMap::new(),
            candidates: HashMap::new(),
            channel_version: None,
        };
        let plan = InstallPlan { actions: vec![action], warnings: vec![] };
        assert!(install_plan(&plan, &fetcher, &paths).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
