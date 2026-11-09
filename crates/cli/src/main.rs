//! `valenced` — developer CLI for the Rust Valenced runtime.
//!
//! Replaces the Gradle/Deno fragments for everyday tasks:
//! ```text
//! valenced plugin validate ./manifest.json
//! valenced theme  validate ./Themes/DARK/Azure Blue AMOLED.json
//! valenced theme  report   ./Themes/DARK/Azure Blue AMOLED.json
//! valenced repo   validate ./repo-index.json --url https://example.com/plugins
//! valenced bundle validate ./bundle-manifest.json
//! valenced ffi    self-test
//! ```
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::collections::HashMap;
use std::path::PathBuf;

use valenced_core::manifest::{parse_bundle_manifest, parse_external_manifest};
use valenced_core::repos::parse_repo_index;
use valenced_core::theme::parse_theme;
use valenced_core::version::{Version, VersionRange};

trait StringResultExt<T> {
    fn with_context_string(self, ctx: impl Into<String>) -> Result<T>;
}

impl<T> StringResultExt<T> for Result<T, String> {
    fn with_context_string(self, ctx: impl Into<String>) -> Result<T> {
        self.map_err(|e| anyhow::anyhow!("{}: {e}", ctx.into()))
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "valenced",
    version,
    about = "Valenced Rust runtime — developer CLI"
)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Validate a plugin manifest.json")]
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    #[command(about = "Validate / report a theme JSON (discord-themes compatible)")]
    Theme {
        #[command(subcommand)]
        action: ThemeAction,
    },
    #[command(about = "Validate a repo index.json or a bundle manifest.json")]
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    #[command(about = "Validate a Valenced bundle manifest (loaders)")]
    Bundle {
        #[command(subcommand)]
        action: BundleAction,
    },
    #[command(about = "Check a version against a range")]
    Version {
        #[command(subcommand)]
        action: VersionAction,
    },
    #[command(about = "FFI / runtime self-checks")]
    Ffi {
        #[command(subcommand)]
        action: FfiAction,
    },
}

#[derive(Subcommand, Debug)]
enum PluginAction {
    Validate {
        path: PathBuf,
    },
    List {
        #[arg(long, default_value = "/tmp/valenced")]
        data_dir: String,
    },
    #[command(about = "Scaffold a new Rust/WASM plugin (TS -> Rust migration)")]
    New {
        /// Plugin id (e.g. `com.example.hello`).
        id: String,
        /// Display name.
        #[arg(long, default_value = "Hello")]
        name: String,
        /// Output directory.
        out: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ThemeAction {
    Validate { path: PathBuf },
    Report { path: PathBuf },
    Css { path: PathBuf },
}

#[derive(Subcommand, Debug)]
enum RepoAction {
    Validate {
        path: PathBuf,
        #[arg(long, default_value = "https://example.com/plugins")]
        url: String,
    },
    Install {
        /// Repo index JSON (file path or https:// URL).
        index: String,
        /// Plugin id to install (e.g. `com.example.hello`).
        id: String,
        #[arg(long, default_value = "/tmp/valenced")]
        data_dir: String,
        /// Repo URL identity (defaults to the index path/url).
        #[arg(long)]
        repo_url: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum BundleAction {
    Validate {
        path: PathBuf,
    },
    #[command(
        about = "Generate bundle manifest from bundle/src/plugins (replaces scripts/manifest.ts)"
    )]
    Generate {
        /// Plugins dir (`bundle/src/plugins`).
        plugins_dir: PathBuf,
        /// Bundle version (e.g. `1.3.0` from bundle/package.json).
        #[arg(long, default_value = "1.0.0")]
        version: String,
        /// Dev mode (includes devOnly plugins, enables dev defaults).
        #[arg(long, default_value_t = false)]
        dev: bool,
        /// Output file (`-` for stdout).
        #[arg(long, default_value = "-")]
        out: String,
        /// Default repo URL for stubs (`VALENCED_DEFAULT_PLUGIN_REPOSITORY_URL`).
        #[arg(long)]
        default_repo: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum VersionAction {
    Check { version: String, range: String },
    Order { versions: Vec<String> },
}

#[derive(Subcommand, Debug)]
enum FfiAction {
    SelfTest,
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::Plugin { action } => match action {
            PluginAction::Validate { path } => {
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let m = parse_external_manifest(&json).with_context_string("invalid manifest")?;
                let pm = m
                    .to_plugin_manifest()
                    .with_context_string("convert manifest")?;
                println!(
                    "✓ {} {} (dist: {})",
                    pm.id,
                    pm.version,
                    if m.dist.as_ref().is_some_and(|d| d.wasm.is_some()) {
                        "wasm"
                    } else {
                        "script"
                    }
                );
                Ok(())
            }
            PluginAction::List { data_dir } => {
                let paths = valenced_core::paths::PluginPaths::new(&data_dir);
                let mut reg = valenced_runtime::Registry::new();
                let (loaded, skipped) = reg.load_from_dist(&paths);
                if loaded.is_empty() {
                    println!("(no plugins installed under {})", paths.dist().display());
                } else {
                    for id in &loaded {
                        let m = reg.manifest(id).unwrap();
                        println!("✓ {} {}", m.id, m.version);
                    }
                }
                for (dir, why) in skipped {
                    eprintln!("  ! skipped {dir}: {why}");
                }
                Ok(())
            }
            PluginAction::New { id, name, out } => {
                valenced_core::manifest::require_valid_plugin_id(&id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                std::fs::create_dir_all(out.join("src"))
                    .with_context(|| format!("mkdir {}", out.display()))?;
                let manifest = serde_json::json!({
                    "format": 1,
                    "id": id,
                    "name": name,
                    "description": "TODO: describe your plugin",
                    "author": "TODO <0>",
                    "contributors": [],
                    "icon": null,
                    "version": "1.0.0",
                    "dependencies": {
                        "valenced.api": { "version": ">=1 <2" },
                        "discord": { "version": "*" }
                    },
                    "dist": { "wasm": "plugin.wasm" }
                });
                std::fs::write(
                    out.join("manifest.json"),
                    serde_json::to_string_pretty(&manifest)?,
                )?;
                let cargo_toml = format!(
                    r#"[package]
name = "{}"
version = "1.0.0"
edition = "2024"
license = "GPL-3.0-only"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
serde_json = "1"
valenced-core = {{ path = "{}", package = "valenced-core" }}
valenced-runtime = {{ path = "{}", package = "valenced-runtime" }}
"#,
                    id.replace('.', "-"),
                    env!("CARGO_MANIFEST_DIR"),
                    env!("CARGO_MANIFEST_DIR")
                );
                // Paths are illustrative; users copy into their repo and fix.
                let cargo_toml = cargo_toml.replace(
                    &format!("{}/crates", env!("CARGO_MANIFEST_DIR")),
                    "../../crates",
                );
                std::fs::write(out.join("Cargo.toml"), &cargo_toml)?;
                let lib_rs = format!(
                    r#"use std::collections::HashMap;
use valenced_core::manifest::{{PluginDependency, PluginManifest}};
use valenced_core::version::{{Version, VersionRange}};
use valenced_runtime::plugin::{{Plugin, PluginContext, RuntimeError}};

fn manifest() -> PluginManifest {{
    let mut deps = HashMap::new();
    deps.insert("valenced.api".into(), PluginDependency {{ version: VersionRange::parse(">=1 <2").unwrap(), optional: false }});
    deps.insert("discord".into(), PluginDependency {{ version: VersionRange::any(), optional: false }});
    PluginManifest {{
        id: "{id}".into(),
        name: "{name}".into(),
        description: String::new(),
        author: String::new(),
        contributors: vec![],
        icon: None,
        dependencies: deps,
        version: Version::parse("1.0.0").unwrap(),
    }}
}}

pub struct {Crate} {{}}

impl Plugin for {Crate} {{
    fn manifest(&self) -> &PluginManifest {{ todo!() }}
    fn start(&mut self, _ctx: &mut PluginContext) -> Result<(), RuntimeError> {{ Ok(()) }}
}}
"#,
                    Crate = name
                        .replace(' ', "")
                        .replace('-', "_")
                        .to_lowercase()
                        .chars()
                        .enumerate()
                        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
                        .collect::<String>()
                );
                // The todo!() above is replaced at build time by a static manifest;
                // kept minimal so `cargo test` in the template is a starting point.
                std::fs::write(out.join("src/lib.rs"), &lib_rs)?;
                println!("✓ scaffolded plugin {id} → {}", out.display());
                Ok(())
            }
        },
        Commands::Theme { action } => match action {
            ThemeAction::Validate { path } => {
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let t = parse_theme(&json).with_context_string("invalid theme")?;
                println!("✓ theme '{}' ({} colors)", t.name, t.semantic_colors.len());
                Ok(())
            }
            ThemeAction::Report { path } => {
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let t = parse_theme(&json).with_context_string("invalid theme")?;
                println!("{}", serde_json::to_string_pretty(&t.report())?);
                Ok(())
            }
            ThemeAction::Css { path } => {
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let t = parse_theme(&json).with_context_string("invalid theme")?;
                println!("{}", t.css_variables());
                Ok(())
            }
        },
        Commands::Repo { action } => match action {
            RepoAction::Validate { path, url } => {
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let (index, warnings) =
                    parse_repo_index(&json, &url).with_context_string("invalid repo index")?;
                println!(
                    "✓ repo '{}' ({} plugins, {} warnings)",
                    index.name,
                    index.plugins.len(),
                    warnings.len()
                );
                for w in warnings {
                    eprintln!("  ! {w}");
                }
                Ok(())
            }
            RepoAction::Install {
                index,
                id,
                data_dir,
                repo_url,
            } => {
                use valenced_core::install::{UreqFetcher, install_one};
                use valenced_core::resolver::ResolveRequest;
                // Index may be a local file path or an https:// URL.
                let index_bytes = if index.starts_with("http") {
                    use valenced_core::install::Fetcher;
                    UreqFetcher.fetch(&index).map_err(|e| anyhow::anyhow!(e))?
                } else {
                    std::fs::read(&index).with_context(|| format!("read {index}"))?
                };
                let index_json = String::from_utf8(index_bytes)
                    .map_err(|e| anyhow::anyhow!("index not utf8: {e}"))?;
                let url = repo_url.unwrap_or_else(|| index.clone());
                let (repo_index, warnings) = parse_repo_index(&index_json, &url)
                    .with_context_string("invalid repo index")?;
                for w in warnings {
                    eprintln!("  ! {w}");
                }
                let paths = valenced_core::paths::PluginPaths::new(&data_dir);
                let installed: std::collections::HashMap<String, Version> = [
                    ("valenced.api".into(), Version::parse("1.0.0").unwrap()),
                    ("discord".into(), Version::parse("350.2").unwrap()),
                ]
                .into_iter()
                .collect();
                let fetcher = UreqFetcher;
                let opts = ResolveRequest::new(&id);
                let plan = install_one(&id, &url, &repo_index, &installed, opts, &fetcher, &paths)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if plan.actions.is_empty() {
                    println!("(already up to date / no action for {id})");
                } else {
                    for a in &plan.actions {
                        println!("✓ installed {}@{} from {}", a.id, a.version, a.repo);
                    }
                }
                for w in plan.warnings {
                    eprintln!("  ! {w}");
                }
                Ok(())
            }
        },
        Commands::Bundle { action } => match action {
            BundleAction::Validate { path } => {
                let json = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                let m = parse_bundle_manifest(&json).with_context(|| "invalid bundle manifest")?;
                let count = m.plugin_manifests().len();
                println!("✓ bundle {} ({} plugins)", m.version, count);
                Ok(())
            }
            BundleAction::Generate {
                plugins_dir,
                version,
                dev,
                out,
                default_repo,
            } => {
                let manifest = valenced_core::bundle_builder::generate_bundle_manifest(
                    &plugins_dir,
                    &version,
                    dev,
                    default_repo.as_deref(),
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                let json = serde_json::to_string_pretty(&manifest)?;
                if out == "-" {
                    println!("{json}");
                } else {
                    std::fs::write(&out, &json).with_context(|| format!("write {out}"))?;
                    println!(
                        "✓ generated bundle {} ({} plugins) → {out}",
                        manifest.version,
                        manifest.plugins.len()
                    );
                }
                Ok(())
            }
        },
        Commands::Version { action } => match action {
            VersionAction::Check { version, range } => {
                let v = Version::parse(&version).with_context_string("invalid version")?;
                let r = VersionRange::parse(&range).with_context_string("invalid range")?;
                println!("{} satisfies {}: {}", v, r, r.satisfies(&v));
                Ok(())
            }
            VersionAction::Order { versions } => {
                let mut vs: Vec<Version> = versions
                    .iter()
                    .map(|s| Version::parse(s))
                    .collect::<Result<_, _>>()
                    .with_context_string("invalid version")?;
                vs.sort();
                let strs: Vec<String> = vs.iter().map(|v| v.to_string()).collect();
                println!("{}", strs.join(" < "));
                Ok(())
            }
        },
        Commands::Ffi { action } => match action {
            FfiAction::SelfTest => {
                // Exercise core paths the FFI wraps (keeps the promises honest).
                let v = Version::parse("1.0.0-beta2").with_context_string("version")?;
                let r = VersionRange::parse(">=1.0 <2").with_context_string("range")?;
                assert!(r.satisfies(&v));
                let json = r#"{"format":1,"id":"com.example.selftest","name":"SelfTest","version":"1.0.0","dependencies":{"valenced.api":{},"discord":{}}}"#;
                parse_external_manifest(json).with_context_string("manifest")?;
                println!("✓ ffi self-test passed (core + runtime linked)");
                Ok(())
            }
        },
    }
}
