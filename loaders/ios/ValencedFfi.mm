// Example call site in PluginLoader.mm:
//   char* out = valenced_manifest_validate([json UTF8String]);
//   if (!out) { /* log + recovery */ }
//   NSString* result = [NSString stringWithUTF8String:out];
//   valenced_free_string(out);
#import "ValencedFfi.h"
