# Changelog

## v0.1.0 — 2026-11-09

First Pure Rust rebuild of Valenced (Phoenix split: thin native loaders + shared Rust host).

### Core (`valenced-core`)
- `Version` / `VersionRange` with exact `Version.kt` ordering (digit-run labels, integer-only satisfies)
- Manifests: `ExternalManifest` / `BundleManifest` / `PluginManifest` (reserved `valenced.api` + `discord`, icon/id validation)
- Repo indexes: sanitize (drop bad versions/plugins/channels with warnings)
- Resolver: `resolve_install` (pinned repos, channels, held, optional skip, candidates/breaks)
- States/paths/atomic IO: `PluginFlag` bitmask + JS payload, `PluginPaths`, `Files.kt` atomic writes
- Themes: `semanticColors` validator + `css_variables` + HTML `preview_html`
- Bundle: `bundle_builder` (manifest scan) + `types_gen` (import maps)
- Install: fetch → sha256/size verify → ZIP extract → `sources.json`

### Runtime (`valenced-runtime`)
- `Plugin` lifecycle (preInit/init/start/stop) + `cleanup`/`decorate`
- `Patcher` hook registry (before LIFO, after FIFO, instead last-wins)
- `DiscordApi` + `FileSystemHost` (modules/assets/storage, `FileModule.writeFile`)
- `Registry` (topo order, `load_from_dist`)
- WASM host (`wasmi`) + JS-compat (`boa_engine` `valenced` global + `eval`)

### FFI / loaders
- `valenced-ffi` C ABI: `version_parse`, `version_satisfies`, `manifest_validate`, `repo_parse`, `theme_validate`, `resolve_install`, `abi_version`
- Android (JNI) + iOS (ObjC) shims in `loaders/`

### CLI (`valenced`)
- `plugin validate|list|start|new`, `theme validate|report|css|preview`, `repo validate|install|refresh|list`, `bundle validate|generate|types`, `version check|order`, `ffi self-test|host-check`

### Desktop (`valenced-desktop`)
- Headless `theme-report` / `dev-snippet`; GPUI window behind `--features gpui`
- Branding: `assets/logo.svg` on Discord `#EF2538`

### Mobile
- `cargo check -p valenced-ffi --target aarch64-linux-android` (Termux-proven) + `aarch64-apple-ios` in CI

### Docs + CI
- `README`, `docs/ARCHITECTURE`/`MIGRATING_FROM_TS`/`FFI_GUIDE`/`PHOENIX_NOTES`/`MOBILE_BUILDS`/`E2E_INSTALL`
- GitHub Actions: `test` (workspace + js-compat + wasm + clippy `-D warnings`) + `ios` check

## v0.2.0 — 2026-11-09

Release engineering + the first downloadable binaries.

- `valenced theme preview` (HTML grid), `ffi host-check`, `repo refresh/list`, `plugin start`, `bundle types`, FFI `resolve_install`
- `#EF2538` branding (README, logo, license shield) + docs theme previews
- Release pipeline: per-target assets (`valenced-<target>[.exe]`, `valenced-desktop-<target>[.exe]`) + `sha256sums-<target>.txt` for Linux x86_64, macOS x86_64/aarch64, Windows x86_64
- `libc` pinned `0.2.189` in committed `Cargo.lock` (GPUI/`xattr` `ENOATTR` fix), `--locked` release builds
- CI: workspace + js-compat + wasm + clippy `-D warnings`, iOS check

Install: download the asset for your platform from [Releases](https://github.com/DEAD1nsane/valenced-rust/releases/tag/v0.2.0), verify against `sha256sums-<target>.txt`, `chmod +x` on Linux/macOS.
