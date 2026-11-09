// Thin Android shim for `valenced-ffi` (C ABI).
// Link `libvalenced_ffi.so` for arm64-v8a / armeabi-v7a into the Xposed APK,
// then call `ValencedFfi.nativeManifestValidate(json)` from `PluginLoader.kt`.
package app.valenced.rust

object ValencedFfi {
    init { System.loadLibrary("valenced_ffi") }

    external fun nativeAbiVersion(): Long
    external fun nativeVersionParse(input: String): String
    external fun nativeVersionSatisfies(version: String, range: String): String
    external fun nativeManifestValidate(json: String): String
    external fun nativeRepoParse(url: String, json: String): String
    external fun nativeThemeValidate(json: String): String
    external fun nativeFreeString(ptr: Long)
}

// Example call site (in PluginLoader.kt):
//   val out = ValencedFfi.nativeManifestValidate(manifestJson)
//   val obj = JSONObject(out)
//   if (!obj.optBoolean("ok", false)) { pluginLog.e("FFI: " + obj.optString("error")) }
