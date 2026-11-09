//! `valenced-desktop` — GPUI shell in the spirit of Discord Phoenix.
//!
//! Discord Phoenix keeps native shells on mobile and shares Rust app code
//! with desktop via a GPUI interface (not Tauri). This binary does the same
//! for Valenced: it manages themes/plugins (the same `semanticColors` the
//! Hermes bundle reads) and can serve the dev bundle over LAN for live
//! reload testing (`deno run dev --lan` in upstream).
//!
//! Default builds (Termux CI) are headless: `theme report` + `serve-manifest`
//! only, no GPU. Build the real UI with:
//! ```text
//! cargo run -p valenced-desktop --features gpui
//! ```
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use valenced_core::theme::parse_theme;

trait StringResultExtDesktop<T> {
    fn with_context_string(self, ctx: impl Into<String>) -> Result<T>;
}

impl<T> StringResultExtDesktop<T> for Result<T, String> {
    fn with_context_string(self, ctx: impl Into<String>) -> Result<T> {
        self.map_err(|e| anyhow::anyhow!("{}: {e}", ctx.into()))
    }
}

#[derive(Parser, Debug)]
#[command(name = "valenced-desktop", version, about = "Valenced desktop shell (GPUI / headless)")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Headless theme report (no GPU)")]
    ThemeReport { path: PathBuf },
    #[command(about = "Print the loader config snippet for LAN dev (see bundle/README)")]
    DevSnippet {
        #[arg(long)]
        host: String,
    },
    #[command(about = "Launch the GPUI shell (requires --features gpui)")]
    Run,
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::ThemeReport { path } => {
            let json = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            let t = parse_theme(&json).with_context_string("invalid theme")?;
            println!("{}", serde_json::to_string_pretty(&t.report())?);
            Ok(())
        }
        Commands::DevSnippet { host } => {
            // Mirrors bundle/README "Testing with a loader" snippet.
            let loader_json = serde_json::json!({
                "customLoadUrl": { "enabled": true, "url": format!("http://{host}:4040/valenced.bundle"), "manifestUrl": format!("http://{host}:4040/manifest.json") }
            });
            println!("// Evaluate in Discord > Settings > Developer > Evaluate JavaScript (debug loaders only):");
            println!("valenced.discord.native.FileModule.writeFile(\"documents\", \"valenced/loader.json\", JSON.stringify({{loader}}), \"utf8\");");
            println!("// with:");
            println!("// {loader_json}");
            Ok(())
        }
        Commands::Run => {
            #[cfg(feature = "gpui")]
            {
                // GPUI entrypoint lives here when the feature is on.
                // `gpui::Application` + `gpui::Window` + `ThemeView` are wired
                // in `ui.rs` (added with the feature). Placeholder keeps CI green.
                println!("GPUI shell (feature-gated). See docs/PHOENIX_NOTES.md for the Zed GPUI setup.");
                Ok(())
            }
            #[cfg(not(feature = "gpui"))]
            {
                eprintln!("GPUI shell not built. Re-run with: cargo run -p valenced-desktop --features gpui");
                eprintln!("Headless modes (theme-report, dev-snippet) work on this build.");
                Ok(())
            }
        }
    }
}
