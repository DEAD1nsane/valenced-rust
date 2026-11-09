# End-to-end: local repo install → plugin list

Proof that `resolve_install` → `install_plan` → `Registry::load_from_dist`
works against a real local repo (Termux-friendly, no network required).

## 1. Build a plugin artifact

```python
# Python 3 stdlib only
import json, zipfile, hashlib, pathlib
base = pathlib.Path("/tmp/valenced-e2e/repo")
base.mkdir(parents=True, exist_ok=True)
manifest = {"format":1,"id":"com.example.e2e","name":"E2E","version":"1.0.0",
            "dependencies":{"valenced.api":{},"discord":{}},"dist":{"wasm":"plugin.wasm"}}
with zipfile.ZipFile(base/"e2e.zip","w") as z:
    z.writestr("manifest.json", json.dumps(manifest))
    z.writestr("plugin.wasm", b"wasm-bytes")
sha = hashlib.sha256((base/"e2e.zip").read_bytes()).hexdigest()
size = (base/"e2e.zip").stat().st_size
index = {"format":1,"name":"e2e-repo","plugins":{
    "com.example.e2e":{"name":"E2E","channels":{"latest":"1.0.0"},
        "versions":{"1.0.0":{"url":"http://127.0.0.1:8765/e2e.zip","sha256":sha,"size":size,
            "dependencies":{"valenced.api":{},"discord":{}}}}}}}
(base/"index.json").write_text(json.dumps(index, indent=2))
```

> **Why HTTP, not `file://`?** `RepoIndex` validation (faithful to
> `RepoIndex.kt`) requires absolute `http(s)` URLs — `file://` drops the
> version with a warning. Serve the dir with `python3 -m http.server 8765`.

## 2. Serve + install + list

```sh
cd /tmp/valenced-e2e/repo && python3 -m http.server 8765 &
cargo run -p valenced-cli -- repo validate ./index.json --url http://127.0.0.1:8765/index.json
cargo run -p valenced-cli -- repo install ./index.json com.example.e2e \
  --data-dir /tmp/valenced-e2e/data --repo-url http://127.0.0.1:8765/index.json
cargo run -p valenced-cli -- plugin list --data-dir /tmp/valenced-e2e/data
```

Expected:

```text
✓ repo 'e2e-repo' (1 plugins, 0 warnings)
✓ installed com.example.e2e@1.0.0 from http://127.0.0.1:8765/index.json
✓ com.example.e2e 1.0.0
```

On disk:

```text
data/files/valenced/plugins/
  dist/com.example.e2e/{manifest.json,plugin.wasm}
  repos/sources.json      # { "com.example.e2e": { "repo": …, "hash": …, "channel": "latest" } }
```
