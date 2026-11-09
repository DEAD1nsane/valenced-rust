# Migrating a TS plugin to Rust / WASM

Legacy plugins (all of `valenced-community-plugins`) are
`index.ts` + `manifest.json` with `"dist": { "script": "index.js" }`.
The Rust runtime runs them unchanged via `js-compat` (Boa). To go native,
convert to a Rust crate that compiles to WASM (or `cdylib` for Android
native plugins). `dist` gains a `wasm` field; `script` and `wasm` are
mutually exclusive (see `ExternalManifest::validate`).

## 1. Scaffold

```sh
cp -r examples/example-plugin ~/GitHub/my-plugin
cd ~/GitHub/my-plugin
```

`manifest.json`:

```jsonc
{
  "format": 1,
  "id": "com.example.show-tag",
  "name": "Show Tag",
  "description": "Show usernames next to display names in chat.",
  "author": "You <123> (https://github.com/you)",
  "contributors": [],
  "icon": "AtIcon",
  "version": "1.0.0",
  "dependencies": {
    "valenced.api": { "version": ">=1 <2" },
    "discord":      { "version": "*" }
  },
  "dist": { "wasm": "plugin.wasm" }
}
```

## 2. Lifecycle mapping

| TS (`PluginOptions`) | Rust (`Plugin` trait) |
|---|---|
| `preInit({ modules, patcher, discord, cleanup })` | `pre_init(&mut self, ctx)` — same args via `ctx.unscoped` |
| `init({ decorate, discord })` | `init(&mut self, ctx)` — `ctx.decorate(id, key, json)` |
| `start({ unscoped: { patcher, modules }, plugin })` | `start(&mut self, ctx)` — `ctx.unscoped.discord.module(..)` |
| `stop()` | `stop(&mut self, ctx)` — runs after `ctx.run_cleanups()` |
| `SettingsComponent` | `settings_component() -> Some("my_plugin.settings")` |

Keep helper code pure (no Discord APIs) so it unit-tests on Tiermux/CI:
plug Discord calls through `ctx.unscoped.discord` (a [`DiscordApi`])
and register hooks through the [`Patcher`] (see `patcher.rs` for the
`before`/`after`/`instead` ordering contract).

## 3. Build to WASM

```sh
cargo build --release --target wasm32-unknown-unknown
# or, for Android native plugins:
cargo build --release --target aarch64-linux-android  # needs NDK toolchain
```

Ship the artifact as `plugin.wasm` and point `dist.wasm` at it. The
loader verifies `size` + `sha256` from the repo index exactly like it
does for `index.js`.

## 4. Validate locally

```sh
cargo run -p valenced-cli -- plugin validate ./manifest.json
cargo test -p valenced-runtime --features wasm
```

## 5. JS-compat interim

If the plugin still has TS, ship `"dist": { "script": "index.js" }` and
run `cargo test -p valenced-runtime --features js-compat`. The `JsPluginHost`
exposes a `valenced` global backed by the same `DiscordApi`. Migrate
incrementally: one `patcher.before(...)` at a time, keeping the manifest
valid throughout.
