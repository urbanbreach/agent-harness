#!/usr/bin/env bash
# Capture startup welcome-panel L3 evidence (P0-START-01 / P0-START-02 / P0-COMP-01).
# Reference freeze: run1-startup (120x32, pinned reference binary 883e3dea).
# Harness scenario: type_first_startup helper (TuiMode::Startup, no keystrokes,
# welcome panel settles with bordered actions + an empty bottom pad + the
# empty bordered composer strip; disconnected-provider status stays in the footer).
#
# Output:
#   artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-startup-v24/
#     terminal.png  terminal.txt  terminal-ansi.txt  metadata.json
#
# Requirements: Linux, Chrome/Chromium for PNG (Playwright cache or CHROME_BIN).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="${EVIDENCE_DIR:-artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-startup-v24}"
COLS=120
ROWS=32
FONT_SIZE=15
FONT_FAMILY="${FONT_FAMILY:-Menlo, \"DejaVu Sans Mono\", \"Noto Sans Mono CJK KR\", monospace}"
DWELL_MS="${DWELL_MS:-3500}"
PRE_DWELL_MS="${PRE_DWELL_MS:-400}"
CHROME_BIN="${CHROME_BIN:-${HOME}/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome}"

if [[ ! -x "$CHROME_BIN" ]]; then
  if command -v google-chrome >/dev/null 2>&1; then
    CHROME_BIN="$(command -v google-chrome)"
  elif command -v chromium >/dev/null 2>&1; then
    CHROME_BIN="$(command -v chromium)"
  elif command -v chromium-browser >/dev/null 2>&1; then
    CHROME_BIN="$(command -v chromium-browser)"
  else
    echo "blocked: no Chrome binary for PNG capture (set CHROME_BIN)" >&2
    exit 2
  fi
fi

echo "Building reference_parity_pty_test..."
cargo test -p harness-tui --test reference_parity_pty_test --no-run

BIN="$(
  find target/debug/deps -maxdepth 1 -type f -name 'reference_parity_pty_test-*' ! -name '*.d' -printf '%T@ %p\n' \
    | sort -nr \
    | head -1 \
    | cut -d' ' -f2-
)"
if [[ -z "$BIN" || ! -x "$BIN" ]]; then
  echo "blocked: reference_parity_pty_test binary not found under target/debug/deps" >&2
  exit 2
fi
echo "Helper binary: $BIN"

mkdir -p "$EVIDENCE_DIR"

export HARNESS_TUI_PTY_HELPER_SCENARIO=type_first_startup
export HARNESS_DETERMINISTIC=1
export HARNESS_DISABLE_ANIMATIONS=1
export HARNESS_SEED=42
export TERM=xterm-256color
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export FORCE_COLOR=1
# Avoid NO_COLOR wiping SGR for visual capture.
unset NO_COLOR || true

CMD="$BIN --exact reference_parity_pty_helper_type_first_startup --nocapture"

echo "Capturing startup welcome L3 -> $EVIDENCE_DIR (120x32, fontSize $FONT_SIZE)"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "harness-startup-v24" \
  --command "$CMD" \
  --source-label "pty_helper:type_first_startup" \
  --cols "$COLS" \
  --rows "$ROWS" \
  --font-size "$FONT_SIZE" \
  --font-family "$FONT_FAMILY" \
  --dwell-ms "$DWELL_MS" \
  --pre-dwell-ms "$PRE_DWELL_MS" \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$EVIDENCE_DIR"

# Structural honesty checks (fail closed on welcome chrome).
if [[ ! -f "$EVIDENCE_DIR/terminal.png" ]]; then
  echo "FAIL: terminal.png missing" >&2
  exit 1
fi
if ! grep -q '┌\|╭' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: welcome L3 missing bordered panel chrome" >&2
  exit 1
fi
if ! grep -q 'New worktree' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: welcome L3 missing action rows" >&2
  exit 1
fi
if grep -q 'No provider connected. Use /connect.\|Notices:' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: welcome L3 must keep provider recovery copy out of the measured panel anatomy" >&2
  exit 1
fi
if ! grep -q '❯' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: welcome L3 missing composer glyph" >&2
  exit 1
fi
if ! grep -q 'Provider not connected' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: welcome L3 missing startup footer status row" >&2
  exit 1
fi

# Provenance metadata: provenance validator requires generating_command and
# matches behavior_id/viewport against the rows that own this L3 directory.
python3 - "$EVIDENCE_DIR/metadata.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    meta = json.load(handle)
meta["behavior_id"] = "P0-START-01"
meta["viewport"] = {"cols": 120, "rows": 32}
meta["generating_command"] = meta.get("generating_command") or (
    "lane-capture: scripts/tui-parity/capture-startup-welcome-l3.sh "
    "(web-terminal-visual-qa.mjs, pty_helper:type_first_startup)"
)
with open(path, "w", encoding="utf-8") as handle:
    json.dump(meta, handle, indent=2)
    handle.write("\n")
PY

echo "OK: startup welcome L3 at $EVIDENCE_DIR"
ls -la "$EVIDENCE_DIR"
