#!/usr/bin/env bash
# Runs the GUI from source with hot reload.
set -euo pipefail
gui="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
addon="$gui/native/dist/gpuix-native.linux-x64-gnu.node"
if [ ! -f "$addon" ]; then
  echo "missing $addon; run gui/native/build.sh first" >&2
  exit 1
fi
export NAPI_RS_NATIVE_LIBRARY_PATH="${NAPI_RS_NATIVE_LIBRARY_PATH:-$addon}"
export HARNESS_BIN="${HARNESS_BIN:-$gui/../target/debug/harness}"
exec bun --hot "$gui/src/main.tsx" "$@"
