#!/usr/bin/env bash
# Capture terminal capability L3 evidence for the four TERM-CAP-* rows
# (TERM-CAP-COLOR / TERM-CAP-KEYS / TERM-CAP-MOUSE / TERM-CAP-CLIPBOARD).
#
# Terminal capability rows prove terminal mode-negotiation parity (escape
# sequences), not visual rendering, so the L3 capture is a capability matrix
# receipt rather than a terminal.png render. The capture runs the env-gated
# owner test `terminal_capability_matrix_capture_test.rs`, which derives the
# Harness negotiated mode set from `TerminalCapabilityLeaf` plus the runtime's
# unconditional synchronized-output framing, asserts exact parity with the
# pinned reference binary's enabled modes (fail-closed), and writes a fresh
# `term-cap-matrix.json` receipt. This script relocates that receipt into the
# lane evidence root and writes the provenance `metadata.json` the strict
# provenance validator (reference_parity_provenance) requires.
#
# No Chrome / pixel PNG is needed (journey-style L3+receipt contract).
#
# Usage:
#   EVIDENCE_DIR=<actual/harness-term-cap-v1> bash scripts/tui-parity/capture-term-cap-l3.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="${EVIDENCE_DIR:-artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-term-cap-v1}"
DIR="harness-term-cap-v1"
AUTHORITY_FILE="${AUTHORITY_FILE:-configs/tui-fidelity-reference-authority.json}"

export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export TERM=xterm-256color
export COLORTERM=truecolor

WORK="$(mktemp -d "${TMPDIR:-/tmp}/term-cap-l3-XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
export HARNESS_TERMCAP_ARTIFACT_DIR="$WORK"

echo "Capturing TERM-CAP-* L3 -> ${EVIDENCE_DIR}"
echo "  running owner test: terminal_capability_matrix_capture_test"
cargo nextest run -p harness-tui --test terminal_capability_matrix_capture_test

SRC="${WORK}/${DIR}"
if [[ ! -f "${SRC}/term-cap-matrix.json" ]]; then
  echo "FAIL: capture test did not produce ${SRC}/term-cap-matrix.json" >&2
  exit 1
fi

DEST="${EVIDENCE_DIR:?}"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -a "$SRC"/. "$DEST"/

# Provenance metadata: the validator requires a non-empty generating_command.
# behavior_id/viewport are intentionally omitted (the receipt is shared by all
# four TERM-CAP-* rows that own this L3 directory); behavior_ids documents
# ownership without triggering the single-owner provenance match.
cat >"$DEST/metadata.json" <<EOF
{
  "behavior_ids": [
    "TERM-CAP-COLOR",
    "TERM-CAP-KEYS",
    "TERM-CAP-MOUSE",
    "TERM-CAP-CLIPBOARD"
  ],
  "row_kind": "terminal_capability",
  "surface": "terminal",
  "generating_command": "scripts/tui-parity/capture-term-cap-l3.sh (terminal_capability_matrix_capture_test)",
  "owner_test": "crates/harness-tui/tests/terminal_capability_matrix_capture_test.rs",
  "capture_dir": "${DIR}",
  "parity_receipt": "term-cap-matrix.json"
}
EOF

# Structural honesty checks (fail closed on the parity conclusion).
if [[ ! -f "$DEST/term-cap-matrix.json" ]]; then
  echo "FAIL: TERM-CAP L3 missing term-cap-matrix.json" >&2
  exit 1
fi
if [[ ! -f "$DEST/metadata.json" ]]; then
  echo "FAIL: TERM-CAP L3 metadata.json was not written" >&2
  exit 1
fi
if ! python3 -c '
import json, sys
receipt = json.load(open(sys.argv[1], encoding="utf-8"))
authority = json.load(open(sys.argv[2], encoding="utf-8"))
active = authority["reference"]["binary_sha256"]
if receipt.get("reference_binary_digest") != active:
    sys.exit("capability owner receipt is not bound to the active authority")
if receipt.get("parity") is not True:
    sys.exit("parity conclusion is not true")
' "$DEST/term-cap-matrix.json" "$AUTHORITY_FILE"; then
  echo "FAIL: TERM-CAP parity receipt failed honesty checks" >&2
  exit 1
fi

echo "OK: TERM-CAP-* L3 at $DEST (term-cap-matrix.json + metadata.json)"
