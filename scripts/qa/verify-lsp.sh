#!/usr/bin/env bash
# Real coordinator results -> production terminal renderer -> xterm.js screenshots.
set -euo pipefail
cd "$(dirname "$0")/../.."
export HARNESS_LSP_EVIDENCE_DIR
HARNESS_LSP_EVIDENCE_DIR="$(realpath -m "${1:-.omo/evidence/lsp-$(date -u +%Y%m%dT%H%M%SZ)}")"
case "$HARNESS_LSP_EVIDENCE_DIR" in
  "$PWD"/.omo/evidence/*) ;;
  *) echo 'Evidence must be under .omo/evidence/' >&2; exit 2 ;;
esac
if [[ -d "$HARNESS_LSP_EVIDENCE_DIR/ansi" ]]; then
  echo 'Choose a fresh evidence directory to avoid mixing recordings.' >&2
  exit 2
fi
command -v rust-analyzer >/dev/null
command -v python3 >/dev/null
test -x /usr/bin/chromium
mkdir -p "$HARNESS_LSP_EVIDENCE_DIR"
rust-analyzer --version | tee "$HARNESS_LSP_EVIDENCE_DIR/rust-analyzer-version.txt"
cargo nextest run --locked --profile ci -p harness-tools --lib -E 'test(lsp)' 2>&1 | tee "$HARNESS_LSP_EVIDENCE_DIR/unit.log"
cargo nextest run --locked --profile ci -p harness-tools --test native_code_lsp_test 2>&1 | tee "$HARNESS_LSP_EVIDENCE_DIR/protocol-and-simulation.log"
cargo nextest run --release --locked --profile ci -p harness-tools --test native_code_lsp_test --run-ignored only --success-output immediate 2>&1 | tee "$HARNESS_LSP_EVIDENCE_DIR/rust-analyzer.log"
cargo nextest run --locked --profile ci -p harness-tui --test lsp_diagnostics_capture_test --run-ignored all 2>&1 | tee "$HARNESS_LSP_EVIDENCE_DIR/renderer.log"
node scripts/qa/render-recorded-frames.mjs "$HARNESS_LSP_EVIDENCE_DIR/ansi" "$HARNESS_LSP_EVIDENCE_DIR/xterm"
python3 - "$HARNESS_LSP_EVIDENCE_DIR" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
manifest = json.loads((root / "xterm/manifest.json").read_text())
assert len(manifest["captures"]) == manifest["producerMetadata"]["captures"] == 39
normalize = lambda text: "\n".join(line.rstrip() for line in text.splitlines()).strip()
for capture in manifest["captures"]:
    name = capture["name"]
    expected = (root / "ansi" / f"{name}.txt").read_text()
    screen = json.loads((root / "xterm" / f"{name}.screen.json").read_text())
    assert normalize(screen["text"]) == normalize(expected), name
(root / "xterm-verification.json").write_text(json.dumps({
    "captures": 39, "renderer_text_matches_xterm": True, "widths": [40, 80, 120]
}, indent=2) + "\n")
print("All 39 xterm.js screens match the production renderer text.")
PY
printf 'LSP evidence: %s\n' "$HARNESS_LSP_EVIDENCE_DIR"
