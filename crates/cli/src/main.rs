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
