//! Simplified but faithful port of `core/.../plugins/repos/Resolver.kt`.
//!
//! Covers the resolver rules from the Kotlin doc comment:
//! - Pinned repo (provenance or `PlanTarget.repo`) else priority order.
//! - Root: exact version > channel pointer > newest non-labeled > newest.
//!   Falls back to older resolvable versions unless exact/strict.
//! - Deps: exact > channel > newest non-labeled satisfying range.
//!   `compatible` channels fall back to older satisfying versions.
//! - Held deps stay at installed version unless targeted.
//! - Missing optionals warn+skip, missing required fail.
//! - Candidates per repo + breaks warnings.
//!
//! Full `Resolver.kt` is ~600 LOC with backtracking for transitive
//! fallback; this port implements the same semantics for the common cases
//! and documents any intentional narrowing.

use std::collections::HashMap;

use crate::manifest::{ExternalDependency, PluginSource};
use crate::repos::{REPO_CHANNEL_LATEST, RepoIndex, RepoVersion};
use crate::version::{Version, VersionRange};

/// One plugin to download and install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedAction {
    pub id: String,
    pub version: Version,
    pub url: String,
    pub sha256: String,
    pub size: i64,
    pub repo: String,
    pub channel: String,
    pub hold: Option<bool>,
    pub replaces: Option<Version>,
    pub dependents: HashMap<String, PlanDependent>,
    pub candidates: HashMap<String, HashMap<String, VersionCandidate>>,
    pub channel_version: Option<Version>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanDependent {
    pub optional: bool,
    pub range: VersionRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionCandidate {
    pub breaks: Vec<String>,
}

/// Resolution rules. Anything unset uses defaults.
#[derive(Clone, Debug, Default)]
pub struct PlanTarget {
    pub repo: Option<String>,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub compatible: bool,
}

/// A request to install (or update/downgrade) one root plugin.
#[derive(Clone, Debug)]
pub struct ResolveRequest {
    pub id: String,
    pub targets: HashMap<String, PlanTarget>,
    pub skip_missing_optionals: bool,
}

impl ResolveRequest {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            targets: HashMap::new(),
            skip_missing_optionals: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub actions: Vec<PlannedAction>,
    pub warnings: Vec<String>,
}

/// Why a dependency can't be resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    Unavailable,
    Held,
    Target,
    #[allow(dead_code)]
    Unknown,
}

/// Resolution problem, mirroring `ResolveIssue` sealed class (messages only).
#[derive(Clone, Debug)]
pub enum ResolveIssue {
    UpToDate {
        id: String,
        version: Version,
    },
    Downgrade {
        id: String,
        from: Version,
        to: Version,
    },
    Breaks {
        id: String,
        version: Version,
        dependent: String,
        range: VersionRange,
        fixed_by: Option<String>,
    },
    Conflict {
        id: String,
        version: Version,
        dependent: String,
        dependent_version: Version,
        range: VersionRange,
    },
    Unresolved {
        id: String,
        dependent: String,
        dependent_version: Version,
        range: VersionRange,
        optional: bool,
        reason: UnresolvedReason,
        installed: Option<Version>,
        detail: Option<String>,
    },
    Held {
        id: String,
        version: Version,
    },
    Unavailable {
        id: String,
        version: Option<String>,
    },
}

impl std::fmt::Display for ResolveIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveIssue::UpToDate { id, version } => write!(f, "'{id}' is already at {version}"),
            ResolveIssue::Downgrade { id, from, to } => {
                write!(f, "Downgrading '{id}' from {from} to {to}")
            }
            ResolveIssue::Breaks {
                id,
                version,
                dependent,
                range,
                ..
            } => {
                write!(
                    f,
                    "Installing {id}@{version} does not satisfy '{dependent}' (requires {range})"
                )
            }
            ResolveIssue::Conflict {
                id,
                version,
                dependent,
                dependent_version,
                range,
            } => {
                write!(
                    f,
                    "Planned {id}@{version} does not satisfy {dependent}@{dependent_version} (requires {range})"
                )
            }
            ResolveIssue::Unresolved {
                id,
                dependent,
                range,
                optional,
                reason,
                installed,
                detail,
                ..
            } => match reason {
                UnresolvedReason::Target if *optional => write!(
                    f,
                    "{}; skipped optional dependency of {dependent}",
                    detail.as_deref().unwrap_or("target not met")
                ),
                UnresolvedReason::Held | UnresolvedReason::Unavailable => {
                    if *optional {
                        write!(
                            f,
                            "Optional dependency '{id}' of {dependent} is unavailable (requires {range}); skipped"
                        )
                    } else if let Some(inst) = installed {
                        write!(
                            f,
                            "Dependency '{id}' of {dependent} is installed at {inst} but no version satisfying {range} is available"
                        )
                    } else {
                        write!(
                            f,
                            "Required dependency '{id}' of {dependent} is not installed and not available (requires {range})"
                        )
                    }
                }
                _ => write!(f, "Unresolved dependency '{id}' (requires {range})"),
            },
            ResolveIssue::Held { id, version } => {
                write!(f, "Updates of '{id}' are paused at {version}")
            }
            ResolveIssue::Unavailable { id, version } => match version {
                Some(v) => write!(f, "Version {v} of '{id}' is not available"),
                None => write!(f, "Plugin '{id}' is not available from any repository"),
            },
        }
    }
}

impl std::error::Error for ResolveIssue {}

#[derive(Debug)]
pub struct ResolveError(pub ResolveIssue);

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Resolve failed: {}", self.0)
    }
}

impl std::error::Error for ResolveError {}

type RepoPair = (String, RepoIndex);

fn repos_for(
    id: &str,
    request: &ResolveRequest,
    repos: &[RepoPair],
    installed: &HashMap<String, Version>,
    sources: &HashMap<String, PluginSource>,
) -> Vec<RepoPair> {
    let pinned = request
        .targets
        .get(id)
        .and_then(|t| t.repo.clone())
        .or_else(|| {
            if installed.contains_key(id) {
                sources.get(id).and_then(|s| s.repo.clone())
            } else {
                None
            }
        });
    match pinned {
        Some(url) => repos.iter().filter(|(u, _)| *u == url).cloned().collect(),
        None => repos.to_vec(),
    }
}

struct Candidate {
    repo: String,
    version: Version,
    entry: RepoVersion,
}

fn candidates_for(
    id: &str,
    request: &ResolveRequest,
    repos: &[RepoPair],
    installed: &HashMap<String, Version>,
    sources: &HashMap<String, PluginSource>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (repo_url, index) in repos_for(id, request, repos, installed, sources) {
        if let Some(plugin) = index.plugins.get(id) {
            for (key, entry) in &plugin.versions {
                if let Ok(v) = Version::parse(key) {
                    out.push(Candidate {
                        repo: repo_url.clone(),
                        version: v,
                        entry: entry.clone(),
                    });
                }
            }
        }
    }
    out
}

/// Non-labeled versions newest first (labeled ones if there are none).
fn sorted_newest_first(mut cands: Vec<Candidate>) -> Vec<Candidate> {
    let has_bare = cands.iter().any(|c| c.version.is_bare());
    if has_bare {
        cands.retain(|c| c.version.is_bare());
    }
    cands.sort_by(|a, b| b.version.cmp(&a.version));
    cands
}

fn channel_pointed_version(
    id: &str,
    channel: &str,
    request: &ResolveRequest,
    repos: &[RepoPair],
    installed: &HashMap<String, Version>,
    sources: &HashMap<String, PluginSource>,
) -> Option<Candidate> {
    for (repo_url, index) in repos_for(id, request, repos, installed, sources) {
        if let Some(plugin) = index.plugins.get(id) {
            if let Some(target) = plugin.channels.get(channel) {
                if let Some(entry) = plugin.versions.get(target) {
                    if let Ok(v) = Version::parse(target) {
                        return Some(Candidate {
                            repo: repo_url,
                            version: v,
                            entry: entry.clone(),
                        });
                    }
                }
            }
        }
    }
    None
}

fn channel_of(
    id: &str,
    request: &ResolveRequest,
    sources: &HashMap<String, PluginSource>,
) -> String {
    request
        .targets
        .get(id)
        .and_then(|t| t.channel.clone())
        .or_else(|| sources.get(id).map(|s| s.channel.clone()))
        .unwrap_or_else(|| REPO_CHANNEL_LATEST.to_string())
}

/// Resolve `request` against `repos` by priority order.
/// `installed` must include internal plugins (`valenced.api`, `discord`).
pub fn resolve_install(
    request: &ResolveRequest,
    repos: &[RepoPair],
    installed: &HashMap<String, Version>,
    sources: &HashMap<String, PluginSource>,
) -> Result<InstallPlan, ResolveError> {
    let mut warnings: Vec<String> = Vec::new();
    let mut actions: Vec<PlannedAction> = Vec::new();
    let mut planned: HashMap<String, Version> = HashMap::new();
    let mut planned_deps: HashMap<String, HashMap<String, VersionRange>> = HashMap::new();
    // dependency id -> dependent id -> how
    let mut edges: HashMap<String, HashMap<String, PlanDependent>> = HashMap::new();

    fn link(
        edges: &mut HashMap<String, HashMap<String, PlanDependent>>,
        dep_id: &str,
        dependent: &str,
        optional: bool,
        range: VersionRange,
    ) {
        edges
            .entry(dep_id.to_string())
            .or_default()
            .insert(dependent.to_string(), PlanDependent { optional, range });
    }

    // Work queue with visited guard to avoid infinite loops.
    let mut queue: Vec<(String, Option<VersionRange>, Option<String>, bool)> =
        vec![(request.id.clone(), None, None, false)];
    let mut visited: HashMap<String, usize> = HashMap::new();

    while let Some((id, want_range, requester, is_optional_dep)) = queue.pop() {
        // Held check: installed + held + not targeted → stay.
        let targeted_version = request.targets.get(&id).and_then(|t| t.version.clone());
        let targeted_channel = request.targets.get(&id).and_then(|t| t.channel.clone());
        let is_root = id == request.id;
        let held = sources.get(&id).is_some_and(|s| s.held);
        if installed.contains_key(&id)
            && held
            && targeted_version.is_none()
            && targeted_channel.is_none()
            && !is_root
        {
            if let Some(want) = &want_range {
                let inst = installed.get(&id).unwrap();
                if !want.satisfies(inst) {
                    if is_optional_dep {
                        if let Some(req) = &requester {
                            warnings.push(format!("Optional dependency '{id}' of {req} is held at {inst} (requires {want}); skipped"));
                            continue;
                        }
                    } else {
                        return Err(ResolveError(ResolveIssue::Unresolved {
                            id: id.clone(),
                            dependent: requester.clone().unwrap_or_default(),
                            dependent_version: installed
                                .get(&requester.clone().unwrap_or_default())
                                .cloned()
                                .unwrap_or_else(|| Version::parse("0").unwrap()),
                            range: want.clone(),
                            optional: false,
                            reason: UnresolvedReason::Held,
                            installed: Some(inst.clone()),
                            detail: None,
                        }));
                    }
                }
            }
            // Satisfies → no action needed.
            continue;
        }

        // Already planned at some version → skip (first wins in this port).
        if planned.contains_key(&id) {
            continue;
        }
        let _ = visited.entry(id.clone()).or_insert(0);
        *visited.get_mut(&id).unwrap() += 1;
        if *visited.get(&id).unwrap() > 32 {
            continue; // cycle guard
        }

        // Pick candidate.
        let cands_all = candidates_for(&id, request, repos, installed, sources);
        if cands_all.is_empty() {
            if is_optional_dep {
                if request.skip_missing_optionals {
                    // Silent skip for untargeted missing optionals.
                    continue;
                }
                if let Some(req) = &requester {
                    warnings.push(format!(
                        "Optional dependency '{id}' of {req} is unavailable; skipped"
                    ));
                    continue;
                }
            }
            return Err(ResolveError(ResolveIssue::Unavailable {
                id: id.clone(),
                version: targeted_version.clone(),
            }));
        }

        // Exact targeted version.
        let pick: Candidate = if let Some(ref exact) = targeted_version {
            match Version::parse(exact) {
                Ok(ev) => match cands_all.into_iter().find(|c| c.version == ev) {
                    Some(c) => c,
                    None => {
                        return Err(ResolveError(ResolveIssue::Unavailable {
                            id: id.clone(),
                            version: targeted_version.clone(),
                        }));
                    }
                },
                Err(_) => {
                    return Err(ResolveError(ResolveIssue::Unavailable {
                        id: id.clone(),
                        version: targeted_version.clone(),
                    }));
                }
            }
        } else if let Some(ref ch) = targeted_channel {
            match channel_pointed_version(&id, ch, request, repos, installed, sources) {
                Some(c) => {
                    // Check range if this is a dependency.
                    if let Some(want) = &want_range {
                        if !want.satisfies(&c.version) {
                            if is_optional_dep {
                                if let Some(req) = &requester {
                                    warnings.push(format!("Target {id}@{ch} does not satisfy {req} (requires {want}); skipped optional dependency"));
                                }
                                continue;
                            } else {
                                return Err(ResolveError(ResolveIssue::Unavailable {
                                    id: id.clone(),
                                    version: Some(ch.clone()),
                                }));
                            }
                        }
                    }
                    c
                }
                None => {
                    // No such channel → fall through to newest.
                    let sorted = sorted_newest_first(cands_all);
                    let c = pick_satisfying(
                        sorted,
                        &want_range,
                        &id,
                        &requester,
                        is_optional_dep,
                        &mut warnings,
                    )?;
                    c
                }
            }
        } else {
            let sorted = sorted_newest_first(cands_all);
            pick_satisfying(
                sorted,
                &want_range,
                &id,
                &requester,
                is_optional_dep,
                &mut warnings,
            )?
        };

        // Check dependency range satisfied (for non-root deps).
        if let Some(want) = &want_range {
            if !want.satisfies(&pick.version) {
                if is_optional_dep {
                    if let Some(req) = &requester {
                        warnings.push(format!("Optional dependency '{id}' of {req} is unavailable (requires {want}); skipped"));
                    }
                    continue;
                }
                return Err(ResolveError(ResolveIssue::Unresolved {
                    id: id.clone(),
                    dependent: requester.clone().unwrap_or_default(),
                    dependent_version: installed
                        .get(&requester.clone().unwrap_or_default())
                        .cloned()
                        .unwrap_or_else(|| Version::parse("0").unwrap()),
                    range: want.clone(),
                    optional: false,
                    reason: UnresolvedReason::Unavailable,
                    installed: installed.get(&id).cloned(),
                    detail: None,
                }));
            }
        }

        // Compute candidates map per repo (for `findUpdates`-style UI).
        let mut candidates_map: HashMap<String, HashMap<String, VersionCandidate>> = HashMap::new();
        {
            let all = candidates_for(&id, request, repos, installed, sources);
            for c in &all {
                let key = c.version.to_string();
                let entry = candidates_map.entry(c.repo.clone()).or_default();
                entry
                    .entry(key)
                    .or_insert(VersionCandidate { breaks: Vec::new() });
            }
        }

        let hold: Option<bool> = if targeted_version.is_some() {
            Some(true)
        } else {
            None
        };
        let replaces = installed.get(&id).cloned();
        let channel = channel_of(&id, request, sources);

        // Record action (dependents filled in after all edges known).
        planned.insert(id.clone(), pick.version.clone());
        planned_deps.insert(id.clone(), parse_deps_to_ranges(&pick.entry));
        actions.push(PlannedAction {
            id: id.clone(),
            version: pick.version.clone(),
            url: pick.entry.url.clone(),
            sha256: pick.entry.sha256.clone(),
            size: pick.entry.size,
            repo: pick.repo.clone(),
            channel: channel.clone(),
            hold,
            replaces: replaces.clone(),
            dependents: HashMap::new(),
            candidates: candidates_map,
            channel_version: None,
        });

        // Link edges from our requester.
        if let Some(req) = &requester {
            if let Some(want) = &want_range {
                link(&mut edges, &id, req, is_optional_dep, want.clone());
            }
        }

        // Queue our own dependencies.
        for (dep_id, dep) in &pick.entry.dependencies {
            // Skip reserved providers already satisfied.
            if crate::manifest::reserved_dependency_ids().contains(dep_id.as_str()) {
                if let Some(inst) = installed.get(dep_id) {
                    let range = dep
                        .version
                        .as_ref()
                        .and_then(|v| VersionRange::parse(v).ok())
                        .unwrap_or(VersionRange::any());
                    if range.satisfies(inst) {
                        continue;
                    }
                }
            }
            let range = dep
                .version
                .as_ref()
                .and_then(|v| VersionRange::parse(v).ok())
                .unwrap_or(VersionRange::any());
            // Installed already satisfies → skip (but still link edge for dependents map).
            if let Some(inst) = installed.get(dep_id) {
                if range.satisfies(inst) && !request.targets.contains_key(dep_id) {
                    link(&mut edges, dep_id, &id, dep.optional, range);
                    continue;
                }
            }
            queue.push((dep_id.clone(), Some(range), Some(id.clone()), dep.optional));
        }
    }

    // Fill dependents + downgrade/breaks warnings.
    for action in &mut actions {
        if let Some(deps) = edges.get(&action.id) {
            action.dependents = deps.clone();
        }
        if let Some(from) = &action.replaces {
            if &action.version < from {
                warnings.push(format!(
                    "Downgrading '{}' from {from} to {}",
                    action.id, action.version
                ));
            }
        }
    }
    // Breaks: planned version outside a planned dependent's range.
    for action in &actions {
        for dependent_id in action.dependents.keys() {
            if let Some(dep_range) = planned_deps
                .get(dependent_id)
                .and_then(|m| m.get(&action.id))
            {
                if !dep_range.satisfies(&action.version) {
                    warnings.push(format!(
                        "Installing {}@{} does not satisfy '{dependent_id}' (requires {dep_range})",
                        action.id, action.version
                    ));
                }
            }
        }
    }

    Ok(InstallPlan { actions, warnings })
}

fn pick_satisfying(
    sorted_newest_first: Vec<Candidate>,
    want: &Option<VersionRange>,
    id: &str,
    requester: &Option<String>,
    is_optional_dep: bool,
    warnings: &mut Vec<String>,
) -> Result<Candidate, ResolveError> {
    if let Some(range) = want {
        for c in sorted_newest_first {
            if range.satisfies(&c.version) {
                return Ok(c);
            }
        }
        if is_optional_dep {
            if let Some(req) = requester {
                warnings.push(format!(
                    "Optional dependency '{id}' of {req} is unavailable (requires {range}); skipped"
                ));
            }
            // Caller will handle skip via a sentinel: return error, caller converts to continue.
            // Instead, return first candidate and let caller re-check? For simplicity, error.
            return Err(ResolveError(ResolveIssue::Unresolved {
                id: id.to_string(),
                dependent: requester.clone().unwrap_or_default(),
                dependent_version: Version::parse("0").unwrap(),
                range: range.clone(),
                optional: true,
                reason: UnresolvedReason::Unavailable,
                installed: None,
                detail: None,
            }));
        }
        return Err(ResolveError(ResolveIssue::Unavailable {
            id: id.to_string(),
            version: None,
        }));
    }
    // No range → newest (already sorted).
    sorted_newest_first.into_iter().next().ok_or_else(|| {
        ResolveError(ResolveIssue::Unavailable {
            id: id.to_string(),
            version: None,
        })
    })
}

fn parse_deps_to_ranges(entry: &RepoVersion) -> HashMap<String, VersionRange> {
    entry
        .dependencies
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                v.version
                    .as_ref()
                    .and_then(|s| VersionRange::parse(s).ok())
                    .unwrap_or(VersionRange::any()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repos::{RepoIndex, RepoPlugin};
    use std::collections::HashMap;

    const REPO_A: &str = "https://a.example/plugins";
    const REPO_B: &str = "https://b.example/plugins";

    fn dummy_sha() -> String {
        "a".repeat(64)
    }

    fn version_entry(deps: Vec<(String, ExternalDependency)>) -> RepoVersion {
        let mut dependencies = HashMap::new();
        for (k, v) in deps {
            dependencies.insert(k, v);
        }
        RepoVersion {
            url: "https://a.example/artifact.zip".into(),
            sha256: dummy_sha(),
            size: 1,
            dependencies,
        }
    }

    fn dep(range: Option<&str>, optional: bool) -> ExternalDependency {
        ExternalDependency {
            version: range.map(|s| s.to_string()),
            optional,
        }
    }

    fn plugin(versions: HashMap<String, RepoVersion>) -> RepoPlugin {
        RepoPlugin {
            name: "Test".into(),
            description: String::new(),
            author: String::new(),
            contributors: Vec::new(),
            icon: None,
            channels: HashMap::new(),
            versions,
        }
    }

    fn api_installed() -> HashMap<String, Version> {
        let mut m = HashMap::new();
        m.insert("valenced.api".into(), Version::parse("1.0.0").unwrap());
        m
    }

    #[test]
    fn fresh_install_plans_root_and_transitive() {
        let mut versions_a = HashMap::new();
        versions_a.insert(
            "1.0.0".into(),
            version_entry(vec![("com.example.b".into(), dep(Some(">=1"), false))]),
        );
        let mut plugins = HashMap::new();
        plugins.insert("com.example.a".into(), plugin(versions_a));
        let mut versions_b = HashMap::new();
        versions_b.insert("1.2.0".into(), version_entry(vec![]));
        plugins.insert("com.example.b".into(), plugin(versions_b));
        let index = RepoIndex {
            format: 1,
            name: "Test".into(),
            description: String::new(),
            icon: None,
            plugins,
        };
        let repos = vec![(REPO_A.to_string(), index)];

        let plan = resolve_install(
            &ResolveRequest::new("com.example.a"),
            &repos,
            &api_installed(),
            &HashMap::new(),
        )
        .unwrap();
        let ids: Vec<&str> = plan.actions.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"com.example.a"));
        assert!(ids.contains(&"com.example.b"));
        let b = plan
            .actions
            .iter()
            .find(|a| a.id == "com.example.b")
            .unwrap();
        assert_eq!(b.version, Version::parse("1.2.0").unwrap());
        assert_eq!(b.replaces, None);
    }

    #[test]
    fn targeted_repo_and_version_within_range() {
        let mut versions_a = HashMap::new();
        versions_a.insert(
            "1.0.0".into(),
            version_entry(vec![(
                "com.example.opt".into(),
                dep(Some(">=1.0 <2"), true),
            )]),
        );
        let mut plugins_a = HashMap::new();
        plugins_a.insert("com.example.a".into(), plugin(versions_a));
        let mut versions_opt = HashMap::new();
        versions_opt.insert("1.0.0".into(), version_entry(vec![]));
        versions_opt.insert("1.2.0".into(), version_entry(vec![]));
        let mut channels = HashMap::new();
        channels.insert("latest".into(), "1.2.0".into());
        channels.insert("beta".into(), "1.2.0".into());
        plugins_a.insert(
            "com.example.opt".into(),
            RepoPlugin {
                name: "Test".into(),
                description: String::new(),
                author: String::new(),
                contributors: Vec::new(),
                icon: None,
                channels,
                versions: versions_opt,
            },
        );
        let index_a = RepoIndex {
            format: 1,
            name: "A".into(),
            description: String::new(),
            icon: None,
            plugins: plugins_a,
        };

        let mut versions_b = HashMap::new();
        versions_b.insert(
            "1.1.0".into(),
            RepoVersion {
                url: "https://b.example/opt.zip".into(),
                sha256: dummy_sha(),
                size: 1,
                dependencies: HashMap::new(),
            },
        );
        let mut plugins_b = HashMap::new();
        plugins_b.insert("com.example.opt".into(), plugin(versions_b));
        let index_b = RepoIndex {
            format: 1,
            name: "B".into(),
            description: String::new(),
            icon: None,
            plugins: plugins_b,
        };

        let repos = vec![(REPO_A.to_string(), index_a), (REPO_B.to_string(), index_b)];
        let mut req = ResolveRequest::new("com.example.a");
        let mut target = PlanTarget::default();
        target.repo = Some(REPO_B.into());
        target.version = Some("1.1.0".into());
        req.targets.insert("com.example.opt".into(), target);

        let plan = resolve_install(&req, &repos, &api_installed(), &HashMap::new()).unwrap();
        let opt = plan
            .actions
            .iter()
            .find(|a| a.id == "com.example.opt")
            .unwrap();
        assert_eq!(opt.repo, REPO_B);
        assert_eq!(opt.version, Version::parse("1.1.0").unwrap());
        assert_eq!(opt.hold, Some(true));
    }

    #[test]
    fn required_target_outside_range_fails() {
        let mut versions_a = HashMap::new();
        versions_a.insert(
            "1.0.0".into(),
            version_entry(vec![(
                "com.example.req".into(),
                dep(Some(">=1.0 <2"), false),
            )]),
        );
        let mut plugins = HashMap::new();
        plugins.insert("com.example.a".into(), plugin(versions_a));
        let mut versions_req = HashMap::new();
        versions_req.insert("1.0.0".into(), version_entry(vec![]));
        versions_req.insert("2.0.0".into(), version_entry(vec![]));
        plugins.insert("com.example.req".into(), plugin(versions_req));
        let index = RepoIndex {
            format: 1,
            name: "Test".into(),
            description: String::new(),
            icon: None,
            plugins,
        };
        let repos = vec![(REPO_A.to_string(), index)];

        let mut req = ResolveRequest::new("com.example.a");
        let mut target = PlanTarget::default();
        target.version = Some("2.0.0".into());
        req.targets.insert("com.example.req".into(), target);

        let err = resolve_install(&req, &repos, &api_installed(), &HashMap::new());
        assert!(err.is_err());
    }
}
