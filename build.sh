#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cd "$SCRIPT_DIR"

cargo build --target wasm32-unknown-unknown --release

WASM=target/wasm32-unknown-unknown/release/rct_mvp.wasm
if [[ ! -f "$WASM" ]]; then
  echo "Build failed: $WASM missing" >&2
  exit 1
fi

SIZE=$(wc -c < "$WASM")
echo "wasm size: ${SIZE} bytes"

B64=$(base64 -w0 "$WASM")

# Use python for the substitution to avoid sed issues with very long replacements.
python3 - "$B64" <<'PY'
import sys, pathlib
b64 = sys.argv[1]
tpl = pathlib.Path("index.template.html").read_text()
out = tpl.replace("__WASM_BASE64__", b64)
pathlib.Path("index.html").write_text(out)
print(f"wrote index.html ({len(out)} bytes)")
PY
