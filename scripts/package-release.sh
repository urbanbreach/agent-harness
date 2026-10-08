#!/usr/bin/env bash
# Package one release binary as dist/harness-<target>.tar.gz.
# Usage: scripts/package-release.sh <target-triple> <binary> <out-dir>
# Asset names carry no version so `releases/latest/download/<asset>` stays stable.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <target-triple> <binary> <out-dir>" >&2
  exit 2
fi
target="$1"
binary="$2"
out_dir="$3"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
name="harness-${target}"

if ! file -b "$binary" | grep -q 'statically linked'; then
  echo "refusing to package $binary: it is not statically linked" >&2
  exit 1
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/$name" "$out_dir"
install -m 0755 "$binary" "$stage/$name/harness"
install -m 0644 "$repo_root/LICENSE" "$repo_root/README.md" "$repo_root/CHANGELOG.md" "$stage/$name/"

# Stable archive bytes for identical inputs.
mtime="${SOURCE_DATE_EPOCH:-$(git -C "$repo_root" log -1 --format=%ct)}"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$mtime" \
  -C "$stage" -cf - "$name" | gzip -9n > "$out_dir/$name.tar.gz"
echo "$out_dir/$name.tar.gz"
