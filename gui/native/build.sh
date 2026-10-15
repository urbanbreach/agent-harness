#!/usr/bin/env bash
# Builds the GPUIX native addon used by the desktop GUI.
#
# The JavaScript side comes from the published @gpuix/* 0.10.0 packages. This
# script compiles the matching native crate with the patches in ./patches and
# writes dist/gpuix-native.linux-x64-gnu.node, which the GUI loads through
# NAPI_RS_NATIVE_LIBRARY_PATH.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Outside the repository: a checkout under it would join the Cargo workspace.
src="${HARNESS_GUI_NATIVE_SRC:-${XDG_CACHE_HOME:-$HOME/.cache}/harness-gui-native}"

# @gpuix/native@0.10.0 and the GPUI fork it pins.
GPUIX_URL=https://github.com/remorses/gpuix
GPUIX_REV=9fcd628863e354e9c58019fc3bf38981a1e64158
ZED_URL=https://github.com/remorses/zed
ZED_REV=81c99f816b4a5f69d3c014774068034c24d1d7af

fetch() {
  local dir=$1 url=$2 rev=$3
  if [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null || true)" != "$rev" ]; then
    mkdir -p "$dir"
    git -C "$dir" init -q
    git -C "$dir" fetch -q --depth 1 "$url" "$rev"
    git -C "$dir" checkout -q --force FETCH_HEAD
  fi
}

fetch "$src/gpuix" "$GPUIX_URL" "$GPUIX_REV"
fetch "$src/gpuix/zed" "$ZED_URL" "$ZED_REV"

# Start every build from the pinned tree, then apply our patches in order.
git -C "$src/gpuix" checkout -q -- .
git -C "$src/gpuix" clean -fdq -e zed -e packages/native/target
for patch in "$here"/patches/*.patch; do
  [ -e "$patch" ] || continue
  git -C "$src/gpuix" apply --whitespace=nowarn "$patch"
done

cargo build --release --no-default-features \
  --manifest-path "$src/gpuix/packages/native/Cargo.toml"

mkdir -p "$here/dist"
cp "$src/gpuix/packages/native/target/release/libgpuix_native.so" \
  "$here/dist/gpuix-native.linux-x64-gnu.node"
echo "built $here/dist/gpuix-native.linux-x64-gnu.node"
