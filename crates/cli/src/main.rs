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
#[command(name = "valenced", version, about = "Valenced Rust runtime — developer CLI")]
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
    Validate { path: PathBuf },
    List {
        #[arg(long, default_value = "/tmp/valenced")]
        data_dir: String,
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
    Validate { path: PathBuf },
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
                let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
                let m = parse_external_manifest(&json).with_context_string("invalid manifest")?;
                let pm = m.to_plugin_manifest().with_context_string("convert manifest")?;
                println!("✓ {} {} (dist: {})", pm.id, pm.version, if m.dist.as_ref().is_some_and(|d| d.wasm.is_some()) { "wasm" } else { "script" });
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
        },
        Commands::Theme { action } => match action {
            ThemeAction::Validate { path } => {
                let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
                let t = parse_theme(&json).with_context_string("invalid theme")?;
                println!("✓ theme '{}' ({} colors)", t.name, t.semantic_colors.len());
                Ok(())
            }
            ThemeAction::Report { path } => {
                let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
                let t = parse_theme(&json).with_context_string("invalid theme")?;
                println!("{}", serde_json::to_string_pretty(&t.report())?);
                Ok(())
            }
            ThemeAction::Css { path } => {
                let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
                let t = parse_theme(&json).with_context_string("invalid theme")?;
                println!("{}", t.css_variables());
                Ok(())
            }
        },
        Commands::Repo { action } => match action {
            RepoAction::Validate { path, url } => {
                let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
                let (index, warnings) = parse_repo_index(&json, &url).with_context_string("invalid repo index")?;
                println!("✓ repo '{}' ({} plugins, {} warnings)", index.name, index.plugins.len(), warnings.len());
                for w in warnings {
                    eprintln!("  ! {w}");
                }
                Ok(())
            }
            RepoAction::Install { index, id, data_dir, repo_url } => {
                use valenced_core::install::{UreqFetcher, install_one};
                use valenced_core::resolver::ResolveRequest;
                // Index may be a local file path or an https:// URL.
                let index_bytes = if index.starts_with("http") {
                    use valenced_core::install::Fetcher;
                    UreqFetcher.fetch(&index).map_err(|e| anyhow::anyhow!(e))?
                } else {
                    std::fs::read(&index).with_context(|| format!("read {index}"))?
                };
                let index_json = String::from_utf8(index_bytes).map_err(|e| anyhow::anyhow!("index not utf8: {e}"))?;
                let url = repo_url.unwrap_or_else(|| index.clone());
                let (repo_index, warnings) = parse_repo_index(&index_json, &url).with_context_string("invalid repo index")?;
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
                let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
                let m = parse_bundle_manifest(&json).with_context(|| "invalid bundle manifest")?;
                let count = m.plugin_manifests().len();
                println!("✓ bundle {} ({} plugins)", m.version, count);
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
                let mut vs: Vec<Version> = versions.iter().map(|s| Version::parse(s)).collect::<Result<_, _>>().with_context_string("invalid version")?;
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
