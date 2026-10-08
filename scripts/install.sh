#!/bin/sh
# Install the Harness binary from GitHub Releases.
#
#   curl -fsSL https://github.com/urbanbreach/agent-harness/releases/latest/download/install.sh | sh
#
# Options (flags or environment):
#   --version vX.Y.Z   HARNESS_VERSION       install a specific release (default: latest)
#   --dir DIR          HARNESS_INSTALL_DIR   install directory (default: ~/.local/bin)
# Uninstall by deleting the installed `harness` file.
set -eu

repo="urbanbreach/agent-harness"
version="${HARNESS_VERSION:-latest}"
install_dir="${HARNESS_INSTALL_DIR:-$HOME/.local/bin}"
# Testing hook: a directory URL holding the release assets.
base_url="${HARNESS_INSTALL_BASE_URL:-}"

fail() {
  printf 'harness install: %s\n' "$*" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; version="$2"; shift 2 ;;
    --dir) [ $# -ge 2 ] || fail "--dir needs a value"; install_dir="$2"; shift 2 ;;
    -h | --help) sed -n '2,10p' "$0" 2>/dev/null || true; exit 0 ;;
    *) fail "unknown option: $1" ;;
  esac
done

[ "$(uname -s)" = Linux ] || fail "Harness supports Linux only (found $(uname -s))"
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) fail "unsupported CPU architecture: $(uname -m)" ;;
esac
asset="harness-${arch}-unknown-linux-musl.tar.gz"

if [ -z "$base_url" ]; then
  case "$version" in
    latest) base_url="https://github.com/${repo}/releases/latest/download" ;;
    v*) base_url="https://github.com/${repo}/releases/download/${version}" ;;
    *) base_url="https://github.com/${repo}/releases/download/v${version}" ;;
  esac
fi

if command -v curl >/dev/null 2>&1; then
  download() { curl -fsSL --retry 3 -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
  download() { wget -q -O "$2" "$1"; }
else
  fail "curl or wget is required"
fi

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "sha256sum or shasum is required to verify the download"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'Downloading %s (%s)\n' "$asset" "$version"
download "${base_url}/${asset}" "$tmp/$asset" || fail "download failed: ${base_url}/${asset}"
download "${base_url}/SHA256SUMS" "$tmp/SHA256SUMS" || fail "download failed: ${base_url}/SHA256SUMS"

expected="$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1 }' "$tmp/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS has no entry for $asset"
actual="$(sha256 "$tmp/$asset")"
[ "$expected" = "$actual" ] || fail "checksum mismatch for $asset (expected $expected, got $actual)"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$install_dir"
# Copy then rename, so a running harness is never overwritten in place.
cp "$tmp/harness-${arch}-unknown-linux-musl/harness" "$install_dir/.harness.new"
chmod 0755 "$install_dir/.harness.new"
mv -f "$install_dir/.harness.new" "$install_dir/harness"

printf 'Installed %s to %s\n' "$("$install_dir/harness" --version)" "$install_dir/harness"
# shellcheck disable=SC2016 # print a literal $PATH for the user to paste
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) printf 'Add %s to PATH, for example:\n  export PATH="%s:$PATH"\n' "$install_dir" "$install_dir" ;;
esac
