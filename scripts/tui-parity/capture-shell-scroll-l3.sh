#!/usr/bin/env bash
# Capture SHELL-SCROLL L3 (scrolled-away-from-follow streaming state).
# Matches reference freeze run1-shell-scroll-pinned-v1 (120x40).
#
# Output:
#   artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-shell-live_scroll-pinned-v1/
#     terminal.png  terminal.txt  terminal-ansi.txt  metadata.json
#
# Requirements: Linux, Chrome/Chromium for PNG (Playwright cache or CHROME_BIN).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="${EVIDENCE_DIR:-artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-shell-live_scroll-pinned-v1}"
COLS=120
ROWS=40
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

export HARNESS_TUI_PTY_HELPER_SCENARIO=live_scroll
export HARNESS_DETERMINISTIC=1
export HARNESS_DISABLE_ANIMATIONS=1
export HARNESS_SEED=42
export TERM=xterm-256color
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export FORCE_COLOR=1
unset NO_COLOR || true

CMD="$BIN --exact pty_helper_live_scroll --nocapture"

echo "Capturing shell-scroll L3 -> $EVIDENCE_DIR (120x40, fontSize $FONT_SIZE)"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "harness-shell-live_scroll-pinned-v1" \
  --command "$CMD" \
  --source-label "pty_helper:live_scroll" \
  --cols "$COLS" \
  --rows "$ROWS" \
  --font-size "$FONT_SIZE" \
  --input "{PageUp}" \
  --dwell-ms "$DWELL_MS" \
  --pre-dwell-ms "$PRE_DWELL_MS" \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$EVIDENCE_DIR"

if ! grep -q 'scroll the parity probe' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: L3 missing user prompt text" >&2
  exit 1
fi
if ! grep -q 'partial streaming row' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: L3 missing assistant response text" >&2
  exit 1
fi
if ! grep -q 'Responding' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: L3 missing Responding streaming indicator" >&2
  exit 1
fi
if ! grep -q 'Shift+Tab:mode' "$EVIDENCE_DIR/terminal.txt"; then
  echo "FAIL: L3 missing standard footer hints" >&2
  exit 1
fi
if [[ ! -f "$EVIDENCE_DIR/terminal.png" ]]; then
  echo "FAIL: terminal.png missing" >&2
  exit 1
fi

echo "OK: shell-scroll L3 at $EVIDENCE_DIR"
ls -la "$EVIDENCE_DIR"
