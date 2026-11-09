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

### GPUI build status

`crates/desktop/src/ui.rs` mirrors the canonical
`gpui-0.2.2/examples/hello_world.rs` shape (`Application::run` →
`cx.open_window` → root view with `Render` + `div()`), using only the
minimal GPUI 0.2 API. It previously imported symbols that no longer exist
in GPUI 0.2 (`Model`, `WindowHandler`, a `Titlebar` element, free-function
`scrollable`, `HttpClient` at the crate root) and created an orphaned
`ThemeView` while displaying a hardcoded empty one — both fixed by the
simplification. `run()`'s signature is unchanged, so `main.rs` is
untouched.

On Linux glibc (x11/wayland) verify with:

```sh
cargo check -p valenced-desktop --features gpui
cargo test -p valenced-desktop   # default, no gpui — must stay green
```

On Termux/Android the `--features gpui` check fails before reaching our
code, inside an upstream transitive dep:
`xattr 0.2.3` (`E0425: cannot find value ENOATTR in crate ::libc`), pulled
via `gpui 0.2.2 → gpui_http_client 0.2.2 → zed-async-tar 0.5.0-zed → xattr`.
Android's bionic `libc` has no `ENOATTR`, so this crate cannot build for
`aarch64-linux-android`. Upstream fix is in `zed-async-tar`/`xattr` (cfg-guard
or drop `xattr` on Android) or in GPUI (don't unconditionally link
`gpui_http_client`). Desktop-only either way: the GPUI window itself
(needs a window manager + GPU) and `ui.rs` (`mod ui` is
`#[cfg(feature = "gpui")]`); default `cargo test` never builds it.

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
