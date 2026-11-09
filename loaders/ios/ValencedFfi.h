// Thin iOS shim for `valenced-ffi` (C ABI). Link `libvalenced_ffi.a` (static)
// from `ios/Sources/Plugins/PluginLoader.mm` — see docs/FFI_GUIDE.md.
#pragma once
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
int64_t valenced_ffi_abi_version(void);
char* valenced_version_parse(const char* input);
char* valenced_version_satisfies(const char* version, const char* range);
char* valenced_manifest_validate(const char* json);
char* valenced_repo_parse(const char* url, const char* json);
char* valenced_theme_validate(const char* json);
char* valenced_resolve_install(const char* request_json, const char* repos_json, const char* installed_json, const char* sources_json);
void  valenced_free_string(char* ptr);
#ifdef __cplusplus
}
#endif
