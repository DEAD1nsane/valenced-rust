<p align="center"><img src="assets/logo.svg" width="128" height="128" alt="Valenced Rust logo (Discord color EF2538)"></p>

# Valenced Rust — Pure Rust runtime for Valenced

A **pure-Rust rebuild** of [Valenced](https://github.com/Valenced/valenced)
in the spirit of Discord's [Project Phoenix](https://youtu.be/y1PDr15raAA):
Discord is dropping Electron + React Native for a Rust client with GPUI,
keeping native shells on mobile and sharing app code with desktop. This repo
does the same for Valenced — thin native loaders + shared Rust core/runtime.

> **Why not “Discord mod in Rust” directly?** Discord mobile runs Hermes
> bytecode (JS/TS). The bundle *must* stay JS/TS to run inside Discord today.
> The Rust win is the **host**: manifest parsing, version resolution, repo
> storage, plugin lifecycle, patching, and the desktop shell. New plugins
> can be Rust/WASM; [existing TS plugins](https://github.com/Valenced/valenced-community-plugins)
> still run via the optional JS-compat bridge.

## Workspace

```text
crates/
  core/     # `valenced-core` — Kotlin core port: Version/Range, manifests,
            # RepoIndex + resolver, slots/states, paths, atomic IO, themes
  runtime/  # `valenced-runtime` — Registry, Plugin lifecycle, Patcher,
            # DiscordApi, WASM host (wasmi), JS-compat (Boa)
  ffi/      # `valenced-ffi` — C ABI for Android Xposed / iOS tweak shims
  cli/      # `valenced` — validate / resolve / repo / theme CLI
  desktop/  # `valenced-desktop` — GPUI shell (headless by default)
examples/
  example-plugin/  # Rust plugin template (manifest.json + src/lib.rs)
docs/
  ARCHITECTURE.md         # Phoenix split + what stays TS/Hermes
  MIGRATING_FROM_TS.md    # TS → Rust/WASM migration guide
  FFI_GUIDE.md            # C ABI → Kotlin/ObjC call sites
  PHOENIX_NOTES.md        # Project Phoenix notes for the GPUI build
```

Upstream mapping (Kotlin → Rust):

| Kotlin (`core/…`) | Rust |
|---|---|
| `plugins/Version.kt` | `crates/core/src/version.rs` (exact ordering/range) |
| `plugins/PluginManifest.kt`, `external/ExternalManifest.kt`, `BundleManifest.kt` | `crates/core/src/manifest.rs` |
| `plugins/repos/RepoIndex.kt`, `PluginSource.kt` | `crates/core/src/repos.rs` |
| `plugins/repos/Resolver.kt` | `crates/core/src/resolver.rs` |
| `plugins/PluginStates.kt`, `PluginPaths.kt`, `core/Files.kt` | `crates/core/src/{states,paths,fs_util}.rs` |
| `bundle/lib/plugins/src/types.ts` (lifecycles) | `crates/runtime/src/plugin.rs` |
| `bundle/lib/patcher` (before/after/instead) | `crates/runtime/src/patcher.rs` |

## Quick start

```sh
# Tests (Termux-friendly, no GPU/C++ needed)
cargo test --workspace

# Validate a community plugin manifest
cargo run -p valenced-cli -- plugin validate ~/GitHub/valenced-community-plugins/plugins/show-tag/manifest.json

# Validate + report one of your themes
cargo run -p valenced-cli -- theme validate ~/GitHub/discord-themes/Themes/DARK/"Azure Blue AMOLED.json"
cargo run -p valenced-cli -- theme report   ~/GitHub/discord-themes/Themes/DARK/"Azure Blue AMOLED.json"
cargo run -p valenced-cli -- theme css      ~/GitHub/discord-themes/Themes/DARK/"Azure Blue AMOLED.json"

# Headless desktop shell helpers
cargo run -p valenced-desktop -- theme-report ~/GitHub/discord-themes/Themes/DARK/"RBW AMOLED.json"
cargo run -p valenced-desktop -- dev-snippet --host 192.168.1.20

# Real GPUI shell (needs a desktop machine + GPU)
cargo run -p valenced-desktop --features gpui -- run

# Legacy TS plugins under the Rust host
cargo test -p valenced-runtime --features js-compat

# WASM plugins
cargo test -p valenced-runtime --features wasm
```

## Plugin formats

```jsonc
// manifest.json — exactly one of dist.script / dist.wasm
{
  "format": 1,
  "id": "com.example.hello",
  "name": "Hello",
  "version": "1.0.0",
  "author": "You <123> (https://example.com)",
  "dependencies": {
    "valenced.api": { "version": ">=1 <2" },  // MUST
    "discord":      { "version": "*" }          // MUST
  },
  "dist": { "wasm": "plugin.wasm" }  // or { "script": "index.js" } for legacy
}
```

See `examples/example-plugin/` for the Rust template and
`docs/MIGRATING_FROM_TS.md` for converting `index.ts` → `src/lib.rs`.

## Themes

The Themes API (`valenced.settings.themes` + `api.themes`) is unchanged:
color themes use `semanticColors` + `plus.iconpack`, font packs and icon
packs resolve the same way. `valenced theme css` renders the exact CSS
variables the GPUI preview shows; the mobile bundle maps them via the
existing provider plugin — no on-device change needed.

## FFI (Android / iOS loaders)

The loaders stay as thin shims (Android Xposed `Main.kt` / iOS `Tweak.xm`
hooking `RCTHost`) + calls into `valenced-ffi`:

```c
// Android (JNI) / iOS (C++)
valenced_version_parse("1.2.0-rc");
valenced_manifest_validate(manifestJson);
valenced_repo_parse(url, indexJson);
valenced_theme_validate(themeJson);
valenced_free_string(ptr);
```

See `docs/FFI_GUIDE.md` for Kotlin/ObjC snippets and the ABI versioning
policy.

## License

GPL-3.0 (same as upstream Valenced). `android/patches` stays AGPL where
noted in upstream `NOTICE`.
