#!/usr/bin/env bash
set -euo pipefail

checkout="$(realpath "${1:?usage: build-probe.sh CHECKOUT OUTPUT}")"
output="$(realpath -m "${2:?usage: build-probe.sh CHECKOUT OUTPUT}")"
probe="$(dirname "$(realpath "$0")")/probe.rs"
manifest="${output}.cargo.jsonl"
mkdir -p "$(dirname "$output")"
cargo build --manifest-path "$checkout/Cargo.toml" --release --locked -p harness \
  --message-format=json > "$manifest"
target="$(cargo metadata --manifest-path "$checkout/Cargo.toml" --no-deps --format-version=1 | jq -r .target_directory)"
externs=()
for crate in harness_core harness_tui harness_providers harness_tools ratatui tokio tokio_stream serde_json tempfile; do
  artifact="$(jq -rs --arg crate "$crate" '[.[] | select(.reason == "compiler-artifact" and .target.name == $crate) | .filenames[] | select(endswith(".rlib"))] | unique | if length == 1 then .[0] else error("ambiguous or missing artifact: " + $crate) end' "$manifest")"
  externs+=(--extern "$crate=$artifact")
done
AUDIT_SSE_SOURCE="$checkout/crates/harness-providers/src/openai/sse.rs" \
  rustc --edition=2021 -C opt-level=3 -C debuginfo=1 -C strip=none \
  -C lto=fat -C codegen-units=1 -L "dependency=$target/release/deps" \
  "${externs[@]}" "$probe" -o "$output"
