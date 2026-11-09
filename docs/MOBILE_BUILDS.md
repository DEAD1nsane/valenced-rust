# Mobile builds — Android / iOS

Valenced is a **Discord mobile** mod. Mobile does **not** use GPUI — it
uses the Hermes bundle + thin native loaders + this crate's C ABI.

## Targets

| Target | What | Output |
|---|---|---|
| `aarch64-linux-android` | Android Xposed module `libvalenced_ffi.so` | `.so` per ABI (`arm64-v8a`) |
| `aarch64-apple-ios` | iOS tweak `libvalenced_ffi.a` | static lib linked into tweak |

Requires NDK / Xcode — not buildable in Termux CI (same as upstream
`android/` + `ios/` today). `cargo test --workspace` (host) covers all
logic; device builds only need the FFI artifact.

## Android (NDK)

```sh
# Terminal app + NDK (§36+). rustup target add aarch64-linux-android
rustup target add aarch64-linux-android
cargo build -p valenced-ffi --release --target aarch64-linux-android
# → target/aarch64-linux-android/release/libvalenced_ffi.so
```

Copy the `.so` into the APK's `jniLibs/arm64-v8a/` and call it from
`loaders/android/kotlin/app/valenced/rust/ValencedFfi.kt`
(`System.loadLibrary("valenced_ffi")`). JNI glue is a 30-line forwarder
to the `valenced_*` C symbols — see `docs/FFI_GUIDE.md`.

## iOS (Xcode)

```sh
# macOS + Xcode
rustup target add aarch64-apple-ios
cargo build -p valenced-ffi --release --target aarch64-apple-ios
# → target/aarch64-apple-ios/release/libvalenced_ffi.a
```

Link the `.a` into the tweak (`ios/Makefile`) and include
`loaders/ios/ValencedFfi.h`. The tweak keeps hooking `RCTHost` to run
the Hermes bundle; Rust handles manifests/resolver/themes via FFI.

## What runs where on mobile

```text
Discord (RN/Hermes)
  └─ bundle/ (TS → Hermes)      plugins, themes UI, patcher
  └─ loader shim (Kotlin/ObjC)  hooks RCTHost / classloader, embeds bundle
       └─ libvalenced_ffi (Rust) Version/Range, manifest/repo validate,
                             resolver, install verify, theme validate
```

Pure Rust plugins (WASM) run in the same host via `wasm_host`; legacy
TS plugins run via `js_compat` (Boa) or the Hermes bundle — both share
the manifest + resolver in `valenced-core`.
