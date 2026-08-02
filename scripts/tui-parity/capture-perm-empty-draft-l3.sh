#!/usr/bin/env bash
# Capture SHELL-PERM empty-draft L3 (freeze-pair for run1-perm-proxy-v2 blank draft slot).
# Does NOT change draft-preserve PTY owners (shell_perm_pty / ovl_perm_pty).
#
# Output:
#   artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-pty-perm-empty-draft-120x32-v1/
#     terminal.png  terminal.txt  terminal-ansi.txt  metadata.json
#
# Requirements: Linux, Chrome/Chromium for PNG (Playwright cache or CHROME_BIN).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="${EVIDENCE_DIR:-artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-pty-perm-empty-draft-120x32-v1}"
COLS=120
ROWS=32
FONT_SIZE=15
DWELL_MS="${DWELL_MS:-2500}"
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
    echo "Text-only dump still available via HARNESS_PERM_EMPTY_DRAFT_L3_DUMP + shell_perm_empty_draft_pty." >&2
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

export HARNESS_TUI_PTY_HELPER_SCENARIO=permission_overlay_empty_draft
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

CMD="$BIN --exact pty_helper_permission_overlay_empty_draft --nocapture"

echo "Capturing empty-draft PERM L3 → $EVIDENCE_DIR (120×32, fontSize $FONT_SIZE)"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "harness-pty-perm-empty-draft-120x32-v1" \
  --command "$CMD" \
  --source-label "pty_helper:pty_helper_permission_overlay_empty_draft" \
  --cols "$COLS" \
  --rows "$ROWS" \
  --font-size "$FONT_SIZE" \
  --dwell-ms "$DWELL_MS" \
  --pre-dwell-ms "$PRE_DWELL_MS" \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$EVIDENCE_DIR"

# Structural honesty checks (fail closed on draft chrome).
if grep -q 'Draft preserved' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: empty-draft L3 must not contain 'Draft preserved'" >&2
  exit 1
fi
if grep -q 'keep draft under permission' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: empty-draft L3 must not contain draft-preserve fixture text" >&2
  exit 1
fi
if ! grep -q 'Allow Edit' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: empty-draft L3 missing Allow Edit chrome" >&2
  exit 1
fi
if [[ ! -f "$EVIDENCE_DIR/terminal.png" ]]; then
  echo "FAIL: terminal.png missing" >&2
  exit 1
fi

echo "OK: empty-draft PERM L3 at $EVIDENCE_DIR"
ls -la "$EVIDENCE_DIR"
