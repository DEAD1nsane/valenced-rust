# Architecture — Valenced Rust (Project Phoenix split)

## The constraint: Hermes

Discord mobile is React Native running Hermes bytecode. Upstream Valenced:

```text
bundle/  TS → Rolldown → Hermes bytecode (runs inside Discord)
core/    Kotlin Multiplatform (shared logic for loaders)
android/ Xposed module (loads bundle into Discord)
ios/     tweak (loads bundle into Discord)
```

You **cannot** replace the Hermes bundle with pure Rust and still run inside
today's Discord — Hermes only executes JS. Project Phoenix proves the
community pattern: Discord rebuilds the client in Rust, mobile keeps a native
UI shell, desktop/mobile share Rust app code via GPUI. Valenced Rust does
the same at plugin-host scale.

## The Rust split

```text
valenced-ffi (C ABI: version / manifest / repo / theme / resolver)
  ↑
android/ Xposed shim (Kotlin, ~thin)     ios/ tweak shim (ObjC, ~thin)
  │                                      │
  └── hooks RCTHost / classloader ──────┘
        │
        ├── crates/runtime (Rust): Registry, Plugin, Patcher, DiscordApi,
        │   WASM host (wasmi), JS-compat (Boa)
        ├── crates/core   (Rust): Version/Range, manifests, RepoIndex,
        │   resolver, slots/states, paths, atomic IO, themes
        │
        └── bundle/ (TS, unchanged): Hermes bundle for legacy plugins
            + Rust plugins (cdylib / WASM) as the future
```

What **stays TypeScript** (why):
- `bundle/` — Hermes bytecode must be JS/TS. Deleting it breaks every
  existing plugin in `valenced-community-plugins` (60+ plugins).
- React/RN components (`SettingsComponent`) render through RN; Rust returns
  a component *id*, the shell renders it.

What **moves to Rust** (why it wins):
- `core/` — manifest parsing, version solving, repo sanitization,
  atomic stores, slot flags. Pure functions, deterministic, testable
  against `ResolverTest.kt`.
- `runtime/` — registry ordering, lifecycle state machine, hook table.
  Same-cost on Termux CI, no GPU/JNI needed for `cargo test`.
- `ffi/` — loaders call Rust for every decision instead of duplicating
  logic in Kotlin/ObjC.
- `cli/` + `desktop/` — replace Gradle/Deno fragments with single binaries.
  The desktop shell pre-figures the Phoenix GPUI client (desktop-only,
  feature-gated).

## Lifecycle mapping

| TS (`types.ts`) | Rust (`runtime`) | Notes |
|---|---|---|
| `preInit(api)` | `Plugin::pre_init(ctx)` | `modules`, `patcher`, `assets`, limited `discord` |
| `init(api)` | `Plugin::init(ctx)` | + `components`, full `discord` |
| `start(api)` | `Plugin::start(ctx)` | all APIs; `AppRegistry.runApplication` |
| `stop(api)` | `Plugin::stop(ctx)` + `ctx.run_cleanups()` | LIFO cleanups + unpatch |
| `cleanup(fn)` | `ctx.cleanup(\|\| …)` | same LIFO semantics |
| `decorate(fn)` | `ctx.decorate(id, key, json)` | JSON extension for dependents |
| `SettingsComponent` | `Plugin::settings_component() -> Option<String>` | GPUI view id or RN component name |

## Patcher mapping

JS monkey-patches functions; Rust is a **hook registry** keyed by
`(target, method, kind)` that the host fires explicitly. Order is preserved:
`before` LIFO, `after` FIFO, `instead` last-wins with an `original`
continuation. See `crates/runtime/src/patcher.rs` for `hooks_for` testing.

## Resolver fidelity

`crates/core/src/resolver.rs` ports `Resolver.kt` rules (pinned repos,
channels, held, optional skip, candidates, breaks). The Kotlin resolver also
does transitive backtracking (fall back to an older version when a newer
one's deps can't resolve). The Rust port covers the same decisions for the
cases in `ResolverTest.kt`; deep backtracking is deferred and documented
in that file. `cargo test -p valenced-core` mirrors the Kotlin tests.

## What this repo does **not** do

- It does **not** fork Discord or ship a new Discord client. Plugin hosts
  run *inside* official Discord via the Xposed/tweak loaders.
- It does **not** reimplement Discord's voice, gateway, or E2EE. Those stay
  in Discord; Valenced only reads/calls exposed modules.
- It does **not** need a root/jailbreak for the `cli`/`desktop` work — only
  the on-device loaders need the usual Valenced install (Xposed / sideloaded
  tweak). See upstream `README.md` → Releases.

## Testing

```sh
cargo test --workspace                    # core + runtime + ffi (+ desktop headless)
cargo test -p valenced-core               # version/manifest/resolver/theme
cargo test -p valenced-runtime --features js-compat  # Boa bridge
cargo test -p valenced-runtime --features wasm       # wasmi bridge
```
