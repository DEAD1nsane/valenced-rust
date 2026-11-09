# Project Phoenix notes — desktop shell

Discord [Project Phoenix](https://youtu.be/y1PDr15raAA) (Nov 2026):

- Desktop Electron → native Rust + **GPUI** (not Tauri). The app “looks and
  works identically” but starts fast and uses ~290 MB instead of GBs.
- Mobile keeps a native UI but shares app code with desktop through Rust.
- Early build supports multiple accounts; performance guidance is
  qualitative (“expect better”) until benchmarks exist.

This repo's `crates/desktop` follows that split:

- **Shared Rust core/runtime** — same `valenced-core` (manifests, resolver,
  themes) drives CLI, desktop, and FFI loaders. No reimplementation.
- **Mobile keeps native UI** — the Android/iOS loaders stay thin shims that
  call `valenced-ffi`; the Hermes bundle keeps the on-device RN UI.
- **Desktop GPUI shell** — `valenced-desktop --features gpui` opens a GPU-
  accelerated window (Zed's GPUI) listing plugins/themes, editing
  `semanticColors`, and serving `valenced.bundle` over LAN for debug
  loaders (`dev-snippet` prints the loader config from `bundle/README.md`).

## Why GPUI is feature-gated

GPUI needs a window manager + GPU and does not build in Termux CI.
Default `valenced-desktop` is headless (`theme-report`, `dev-snippet`,
`run` stub). On a desktop machine:

```sh
cargo run -p valenced-desktop --features gpui -- run
```

## Memory / perf posture

Like Phoenix, no benchmark claims without measurements. The Rust host
avoids the JS heap for cold paths (manifest parse, resolve, repo
sanitize), so cold-start work is allocation-light. Run
`cargo bench` (future) + measure `time valenced theme report …` on a
phone vs. the equivalent Deno/Gradle step before making claims.

## Upstreaming

Discord considers upstreaming GPUI patches. For Valenced, theme preview
(`Theme::css_variables`) and the hook-registry (`Patcher`) are the two
surfaces most likely to generalize as a `gpui-valenced` example crate
without forking GPUI — see `docs/ARCHITECTURE.md` for the boundary.
