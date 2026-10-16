#!/usr/bin/env bash
# Builds the GPUIX native addon used by the desktop GUI.
#
# JavaScript packages and native code use the same pinned GPUIX checkout. This
# script compiles the native crate with the patches in ./patches and
# writes dist/gpuix-native.linux-x64-gnu.node, which the GUI loads through
# NAPI_RS_NATIVE_LIBRARY_PATH. `--test` also runs the native crate's tests
# and the patched GPUI list regression.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Outside the repository: a checkout under it would join the Cargo workspace.
src="${HARNESS_GUI_NATIVE_SRC:-${XDG_CACHE_HOME:-$HOME/.cache}/harness-gui-native}"

# Matching GPUIX source and its GPUI fork pin.
GPUIX_URL=https://github.com/remorses/gpuix
GPUIX_REV=6a5c7c142ac32f9eb0caefe0121be995cfc18631
ZED_URL=https://github.com/remorses/zed
ZED_REV=c2e9f882030ca95cee55d206512a0a3ee6cd72b0

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
git -C "$src/gpuix/zed" checkout -q -- .
git -C "$src/gpuix/zed" clean -fdq
for patch in "$here"/patches/zed/*.patch; do
  [ -e "$patch" ] || continue
  git -C "$src/gpuix/zed" apply --whitespace=nowarn "$patch"
done

bun install --cwd "$src/gpuix" --ignore-scripts
bun run --cwd "$src/gpuix/packages/native" build:js
bun run --cwd "$src/gpuix/packages/react" build

# Generate declarations from the same Rust build as the addon.
bun run --cwd "$src/gpuix/packages/native" build:harness
# Copy only published runtime files, never the Rust target directory.
bun -e '
const fs = require("node:fs");
const path = require("node:path");
const [source, destination] = process.argv.slice(1);
for (const name of ["native", "react"]) {
  const sourceDir = path.join(source, "packages", name);
  const targetDir = path.join(destination, name);
  const manifest = JSON.parse(fs.readFileSync(path.join(sourceDir, "package.json"), "utf8"));
  fs.rmSync(targetDir, { recursive: true, force: true });
  fs.mkdirSync(targetDir, { recursive: true });
  for (const entry of manifest.files) {
    const from = path.join(sourceDir, entry);
    if (fs.existsSync(from)) fs.cpSync(from, path.join(targetDir, entry), { recursive: true });
  }
  if (manifest.dependencies?.["@gpuix/native"]) manifest.dependencies["@gpuix/native"] = "file:../native";
  fs.writeFileSync(path.join(targetDir, "package.json"), JSON.stringify(manifest, null, 2) + "\n");
}
' "$src/gpuix" "$here/../vendor/gpuix"

mkdir -p "$here/dist"
# Replace atomically so an already-running renderer keeps its mapped addon intact.
cp "$src/gpuix/packages/native/gpuix-native.linux-x64-gnu.node" \
  "$here/dist/gpuix-native.linux-x64-gnu.node.new"
mv "$here/dist/gpuix-native.linux-x64-gnu.node.new" \
  "$here/dist/gpuix-native.linux-x64-gnu.node"
echo "built $here/dist/gpuix-native.linux-x64-gnu.node"
if [ "${1:-}" = "--test" ]; then
  cargo nextest run --release --manifest-path "$src/gpuix/packages/native/Cargo.toml"
  cargo nextest run --release --manifest-path "$src/gpuix/zed/Cargo.toml" -p gpui \
    test_layout_handler_reports_programmatic_scroll_without_wheel
fi
