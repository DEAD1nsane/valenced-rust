# FFI guide — calling `valenced-ffi` from Android / iOS

`crates/ffi` exports a stable C ABI (`VALENCED_FFI_ABI_VERSION = 1`).
All strings are UTF-8 JSON in/out. Loaders check
`valenced_ffi_abi_version()` at startup and bail with a recovery screen on
mismatch (see upstream `RecoveryCompanion.kt`).

## ABI

```c
typedef char c_char;

// ABI version (call first).
intptr_t valenced_ffi_abi_version(void);

// Version checks.
char* valenced_version_parse(const char* input);
char* valenced_version_satisfies(const char* version, const char* range);

// Manifest / repo / theme validators.
char* valenced_manifest_validate(const char* json);
char* valenced_repo_parse(const char* url, const char* json);
char* valenced_theme_validate(const char* json);

// Ownership: every returned `char*` must be freed exactly once.
void valenced_free_string(char* ptr);
```

Error shape (all functions): `{"ok":false,"code":<i64>,"error":"…"}`.
Success shape: `{"ok":true,"value":…}` (or `{"ok":true,"index":…,"warnings":[…]}`).

## Android (Kotlin / JNI)

Add to `android/src/main/kotlin/app/valenced/xposed/tweaks/plugins/`:

```kotlin
// ValencedFfi.kt — JNI bridge; `libvalenced_ffi.so` ships with the Xposed module.
object ValencedFfi {
    external fun nativeVersionParse(input: String): String
    external fun nativeManifestValidate(json: String): String
    external fun nativeRepoParse(url: String, json: String): String
}

// Usage in PluginLoader.kt (replaces inline Kotlin parsing where possible):
val result = ValencedFfi.nativeManifestValidate(manifestJson)
val obj = JSONObject(result)
if (!obj.optBoolean("ok", false)) {
    pluginLog.e("FFI manifest validate failed: " + obj.optString("error"))
}
```

JNI glue (`ValencedFfi.cpp`) is a 30-line forwarder to the C ABI above —
no Rust types leak into Kotlin. Keep `libvalenced_ffi.so` for
`arm64-v8a` / `armeabi-v7a` in the APK; the C ABI is identical across ABIs.

## iOS (ObjC++)

In `ios/Sources/Plugins/PluginLoader.mm`, link `libvalenced_ffi.a`
(static) or `libvalenced_ffi.dylib`:

```objc
#import "ValencedFfi.h"  // generated C header for the 7 functions above

NSString *json = [NSString stringWithData:data encoding:NSUTF8StringEncoding];
char *out = valenced_manifest_validate([json UTF8String]);
if (!out) { /* log + recovery */ }
NSString *result = [NSString stringWithUTF8String:out];
valenced_free_string(out);
```

## Versioning

- Bugfix / additive: `VALENCED_FFI_ABI_VERSION` unchanged; loaders only
  require `>= 1`.
- Breaking (signature / JSON shape change): bump to `2`, ship new
  `libvalenced_ffi` with the loaders, and fail-closed on mismatch so old
  loaders show Recovery instead of crashing Discord.

## Resolver (not in FFI yet)

`resolve_install` is exposed for `cli` + desktop use today. When the
loaders grow a full “plan install” UI, expose it as
`valenced_resolve_install(requestJson, reposJson, installedJson)`
returning the same `InstallPlan` JSON — the Rust implementation already
matches `Resolver.kt` semantics for `ResolverTest.kt` cases.
