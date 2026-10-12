#!/usr/bin/env bash
# Exercise the release archive and installer with an isolated installation.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
archive="$(realpath "$1")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir "$work/assets" "$work/unpacked"
cp "$archive" "$work/assets/"
(cd "$work/assets" && sha256sum ./*.tar.gz | sed 's|\./||' > SHA256SUMS)
tar -xzf "$archive" -C "$work/unpacked"
package="$(find "$work/unpacked" -mindepth 1 -maxdepth 1 -type d)"
for notice in dependencies.txt syntax.txt acorn.txt rust-stdlib.html musl.txt zig.txt; do
  test -s "$package/LICENSES/$notice"
done
HARNESS_INSTALL_BASE_URL="file://$work/assets" sh "$repo_root/scripts/install.sh" --dir "$work/bin"
cmp "$package/harness" "$work/bin/harness"
cmp "$package/LICENSE" "$work/bin/harness-licenses/LICENSE"
diff -qr "$package/LICENSES" "$work/bin/harness-licenses/third-party"
# A damaged archive must fail verification without replacing an installed binary.
printf 'corrupt\n' >> "$work/assets/$(basename "$archive")"
if HARNESS_INSTALL_BASE_URL="file://$work/assets" sh "$repo_root/scripts/install.sh" --dir "$work/bin" > "$work/rejected.log" 2>&1; then
  echo 'installer accepted a damaged archive' >&2
  exit 1
fi
cmp "$package/harness" "$work/bin/harness"
echo 'Release archive, notices, installation and checksum checks passed'
