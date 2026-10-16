#!/usr/bin/env bash
# Runs the desktop GUI from this checkout. `--hot` reloads on source changes.
# `harness gui` uses it when HARNESS_GUI points here.
set -euo pipefail
gui="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"
addon="$gui/native/dist/gpuix-native.linux-x64-gnu.node"
if [ ! -f "$addon" ]; then
  echo "missing $addon; run gui/native/build.sh first" >&2
  exit 1
fi
export NAPI_RS_NATIVE_LIBRARY_PATH="${NAPI_RS_NATIVE_LIBRARY_PATH:-$addon}"
export HARNESS_BIN="${HARNESS_BIN:-$gui/../target/debug/harness}"
if [ "${1:-}" = "--hot" ]; then
  shift
  exec bun --hot "$gui/src/main.tsx" "$@"
fi
exec bun "$gui/src/main.tsx" "$@"
