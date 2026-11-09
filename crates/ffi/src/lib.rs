//! `valenced-ffi` — C ABI for the thin native loaders.
//!
//! The Android Xposed module and iOS tweak stay thin Kotlin/ObjC shims that
//! hook `RCTHost` / the Android classloader, then call into this crate for:
//! - version parsing / range checks
//! - manifest + repo-index validation
//! - resolver runs
//! - slot state read/write
//!
//! All strings are UTF-8 JSON in/out so the loaders don't need to know the
//! Rust types. See `docs/FFI_GUIDE.md` for the Kotlin/ObjC call sites.
//!
//! # Safety
//! Every exported `extern "C"` function is null-checked.
//! Callers must free returned strings with [`valenced_free_string`].
//! Keep this ABI stable: breaking changes bump `VALENCED_FFI_ABI_VERSION`.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use valenced_core::manifest::parse_external_manifest;
use valenced_core::repos::parse_repo_index;
use valenced_core::theme::parse_theme;
use valenced_core::version::{Version, VersionRange};

/// ABI version. Bump on incompatible changes; loaders check at startup.
pub const VALENCED_FFI_ABI_VERSION: i64 = 1;

unsafe fn cstr_to_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string()) }
}

fn json_out(value: serde_json::Value) -> *mut c_char {
    let s = serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
    CString::new(s)
        .map(|c| c.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn err_out(code: i64, message: impl Into<String>) -> *mut c_char {
    json_out(serde_json::json!({ "ok": false, "code": code, "error": message.into() }))
}

fn ok_out(value: serde_json::Value) -> *mut c_char {
    if value.is_object() && value.get("ok").is_some() {
        json_out(value)
    } else {
        json_out(serde_json::json!({ "ok": true, "value": value }))
    }
}

/// Free a string returned by any `valenced_*` function in this crate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    };
}

fn version_to_json(v: &Version) -> serde_json::Value {
    serde_json::json!({ "nums": v.nums, "label": v.label, "display": v.to_string() })
}

/// Parse a version string. Returns JSON `{"ok":true,"value":{"nums":[…],"label":…}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_version_parse(input: *const c_char) -> *mut c_char {
    let s = unsafe { cstr_to_str(input) };
    let Some(s) = s else {
        return err_out(-2, "null input");
    };
    match Version::parse(&s) {
        Ok(v) => ok_out(version_to_json(&v)),
        Err(e) => err_out(-1, e),
    }
}

/// Check `version` against `range` (`"*"` or `">=1.0 <2"`). Returns `{"ok":true,"value":true|false}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_version_satisfies(
    version_ptr: *const c_char,
    range_ptr: *const c_char,
) -> *mut c_char {
    let v = unsafe { cstr_to_str(version_ptr) };
    let r = unsafe { cstr_to_str(range_ptr) };
    let (Some(vs), Some(rs)) = (v.as_deref(), r.as_deref()) else {
        return err_out(-2, "null input");
    };
    match (Version::parse(vs), VersionRange::parse(rs)) {
        (Ok(v), Ok(range)) => ok_out(serde_json::Value::Bool(range.satisfies(&v))),
        (Err(e), _) => err_out(-1, e),
        (_, Err(e)) => err_out(-1, e),
    }
}

/// Validate `ExternalManifest` JSON. Returns `{"ok":true,"value":{…manifest…}}` or an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_manifest_validate(json_ptr: *const c_char) -> *mut c_char {
    let json = unsafe { cstr_to_str(json_ptr) };
    match json.as_deref() {
        Some(json) => match parse_external_manifest(json) {
            Ok(m) => ok_out(serde_json::to_value(&m).unwrap_or(serde_json::Value::Null)),
            Err(e) => err_out(-1, e),
        },
        None => err_out(-2, "null input"),
    }
}

/// Parse a repo index (`repoUrl`, `json`). Returns `{"ok":true,"value":{index,warnings}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_repo_parse(
    url_ptr: *const c_char,
    json_ptr: *const c_char,
) -> *mut c_char {
    let url = unsafe { cstr_to_str(url_ptr) };
    let json = unsafe { cstr_to_str(json_ptr) };
    match (url.as_deref(), json.as_deref()) {
        (Some(url), Some(json)) => match parse_repo_index(json, url) {
            Ok((index, warnings)) => ok_out(serde_json::json!({
                "index": serde_json::to_value(&index).unwrap_or(serde_json::Value::Null),
                "warnings": warnings,
            })),
            Err(e) => err_out(-1, e),
        },
        _ => err_out(-2, "null input"),
    }
}

/// Validate a theme document. Returns `{"ok":true,"value":{…theme…}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_theme_validate(json_ptr: *const c_char) -> *mut c_char {
    let json = unsafe { cstr_to_str(json_ptr) };
    match json.as_deref() {
        Some(json) => match parse_theme(json) {
            Ok(t) => ok_out(serde_json::to_value(&t).unwrap_or(serde_json::Value::Null)),
            Err(e) => err_out(-1, e),
        },
        None => err_out(-2, "null input"),
    }
}

/// Plan an install from JSON inputs. Inputs:
/// - `requestJson`: `{"id": "...", "targets": {...}, "skipMissingOptionals": false}`
/// - `reposJson`: `[{"url": "...", "index": {...}}]`
/// - `installedJson`: `{"<id>": "1.0.0", ...}` (includes `valenced.api`/`discord`)
/// - `sourcesJson`: `{"<id>": {"repo": ..., "hash": ..., "channel": ..., "held": ...}}`
///
/// Returns `{"ok":true,"value":{"actions":[…],"warnings":[…]}}`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_resolve_install(
    request_ptr: *const c_char,
    repos_ptr: *const c_char,
    installed_ptr: *const c_char,
    sources_ptr: *const c_char,
) -> *mut c_char {
    use std::collections::HashMap;
    use valenced_core::manifest::PluginSource;
    use valenced_core::repos::RepoIndex;
    use valenced_core::resolver::{PlanTarget, ResolveRequest, resolve_install};

    let request_s = unsafe { cstr_to_str(request_ptr) };
    let repos_s = unsafe { cstr_to_str(repos_ptr) };
    let installed_s = unsafe { cstr_to_str(installed_ptr) };
    let sources_s = unsafe { cstr_to_str(sources_ptr) };
    let (Some(req_s), Some(repos_s), Some(inst_s), Some(src_s)) = (
        request_s.as_deref(),
        repos_s.as_deref(),
        installed_s.as_deref(),
        sources_s.as_deref(),
    ) else {
        return err_out(-2, "null input");
    };

    let req_v: serde_json::Value = match serde_json::from_str(req_s) {
        Ok(v) => v,
        Err(e) => return err_out(-1, e.to_string()),
    };
    let id = req_v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return err_out(-1, "request.id missing");
    }
    let mut request = ResolveRequest::new(&id);
    request.skip_missing_optionals = req_v
        .get("skipMissingOptionals")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if let Some(targets) = req_v.get("targets").and_then(|x| x.as_object()) {
        for (tid, t) in targets {
            request.targets.insert(
                tid.clone(),
                PlanTarget {
                    repo: t
                        .get("repo")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    version: t
                        .get("version")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    channel: t
                        .get("channel")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string()),
                    compatible: t
                        .get("compatible")
                        .and_then(|x| x.as_bool())
                        .unwrap_or(false),
                },
            );
        }
    }

    let repos_v: serde_json::Value = match serde_json::from_str(repos_s) {
        Ok(v) => v,
        Err(e) => return err_out(-1, e.to_string()),
    };
    let mut repos: Vec<(String, RepoIndex)> = Vec::new();
    if let Some(arr) = repos_v.as_array() {
        for entry in arr {
            let url = entry
                .get("url")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(index_v) = entry.get("index") {
                if let Ok(index) = serde_json::from_value::<RepoIndex>(index_v.clone()) {
                    repos.push((url, index));
                }
            }
        }
    }

    let inst_v: serde_json::Value = match serde_json::from_str(inst_s) {
        Ok(v) => v,
        Err(e) => return err_out(-1, e.to_string()),
    };
    let mut installed: HashMap<String, Version> = HashMap::new();
    if let Some(obj) = inst_v.as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                let _ = Version::parse(s).map(|parsed| installed.insert(k.clone(), parsed));
            }
        }
    }

    let src_v: serde_json::Value = match serde_json::from_str(src_s) {
        Ok(v) => v,
        Err(e) => return err_out(-1, e.to_string()),
    };
    let sources: HashMap<String, PluginSource> = serde_json::from_value(src_v).unwrap_or_default();

    match resolve_install(&request, &repos, &installed, &sources) {
        Ok(plan) => {
            let actions: Vec<_> = plan
                .actions
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "id": a.id,
                        "version": a.version.to_string(),
                        "url": a.url,
                        "sha256": a.sha256,
                        "size": a.size,
                        "repo": a.repo,
                        "channel": a.channel,
                        "hold": a.hold,
                        "replaces": a.replaces.as_ref().map(|v| v.to_string()),
                    })
                })
                .collect();
            ok_out(serde_json::json!({ "actions": actions, "warnings": plan.warnings }))
        }
        Err(e) => err_out(-1, e.0.to_string()),
    }
}

/// The FFI ABI version, for loader startup checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn valenced_ffi_abi_version() -> i64 {
    VALENCED_FFI_ABI_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn call(ptr: *mut c_char) -> serde_json::Value {
        assert!(!ptr.is_null());
        let v: serde_json::Value =
            serde_json::from_str(unsafe { CStr::from_ptr(ptr).to_str().unwrap() }).unwrap();
        unsafe { valenced_free_string(ptr) };
        v
    }

    #[test]
    fn version_parse_via_ffi() {
        let input = CString::new("1.2.0-rc").unwrap();
        let out = call(unsafe { valenced_version_parse(input.as_ptr()) });
        assert_eq!(out["ok"], true);
        assert_eq!(out["value"]["display"], "1.2.0-rc");
    }

    #[test]
    fn satisfies_via_ffi() {
        let v = CString::new("1.5-rc").unwrap();
        let r = CString::new(">=1.0 <2").unwrap();
        let out = call(unsafe { valenced_version_satisfies(v.as_ptr(), r.as_ptr()) });
        assert_eq!(out["value"], true);
    }

    #[test]
    fn manifest_validate_via_ffi() {
        let json = CString::new(r#"{"format":1,"id":"com.example.hello","name":"Hello","version":"1.0.0","dependencies":{"valenced.api":{},"discord":{}}}"#).unwrap();
        let out = call(unsafe { valenced_manifest_validate(json.as_ptr()) });
        assert_eq!(out["ok"], true);
    }

    #[test]
    fn null_input_is_safe() {
        let out = call(unsafe { valenced_version_parse(std::ptr::null()) });
        assert_eq!(out["ok"], false);
    }
}
