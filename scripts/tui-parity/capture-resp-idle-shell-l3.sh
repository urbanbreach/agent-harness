#!/usr/bin/env bash
# Capture responsive idle-shell L3 evidence for all 7 viewports.
# Reference freeze (run1-resp-*-pinned-v1) shows real HOME idle shell.
# Harness scenario: idle_shell (TuiMode::Live, no events, parity context window).
#
# Output per viewport:
#   artifacts/qa-evidence/20260717-tui-reference-parity/actual/harness-resp-<W>x<H>-pinned-v2/
#     terminal.png  terminal.txt  terminal-ansi.txt  metadata.json
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_BASE="${EVIDENCE_BASE:-artifacts/qa-evidence/20260717-tui-reference-parity/actual}"
FONT_SIZE=15
DWELL_MS="${DWELL_MS:-2500}"
PRE_DWELL_MS="${PRE_DWELL_MS:-400}"
CHROME_BIN="${CHROME_BIN:-${HOME}/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome}"

if [[ ! -x "$CHROME_BIN" ]]; then
  if command -v google-chrome >/dev/null 2>&1; then
    CHROME_BIN="$(command -v google-chrome)"
  elif command -v chromium >/dev/null 2>&1; then
    CHROME_BIN="$(command -v chromium)"
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

export HARNESS_TUI_PTY_HELPER_SCENARIO=idle_shell
export HARNESS_DETERMINISTIC=1
export HARNESS_DISABLE_ANIMATIONS=1
export HARNESS_SEED=42
export TERM=xterm-256color
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export FORCE_COLOR=1
unset NO_COLOR || true

CMD="$BIN --exact pty_helper_idle_shell --nocapture"

VIEWPORTS=(
  "120x50"
  "120x40"
  "100x30"
  "80x24"
  "79x24"
  "60x20"
  "140x40"
)

for VP in "${VIEWPORTS[@]}"; do
  COLS="${VP%x*}"
  ROWS="${VP#*x}"
  EVIDENCE_DIR="${EVIDENCE_BASE}/harness-resp-${VP}-pinned-v2"
  mkdir -p "$EVIDENCE_DIR"

  echo "Capturing idle-shell L3 → $EVIDENCE_DIR (${COLS}×${ROWS})"
  node scripts/tui-parity/web-terminal-visual-qa.mjs \
    --title "harness-resp-${VP}-pinned-v2" \
    --command "$CMD" \
    --source-label "pty_helper:idle_shell" \
    --cols "$COLS" \
    --rows "$ROWS" \
    --font-size "$FONT_SIZE" \
    --dwell-ms "$DWELL_MS" \
    --pre-dwell-ms "$PRE_DWELL_MS" \
    --chrome-bin "$CHROME_BIN" \
    --evidence-dir "$EVIDENCE_DIR"

  if [[ ! -f "$EVIDENCE_DIR/terminal.png" ]]; then
    echo "FAIL: terminal.png missing for ${VP}" >&2
    exit 1
  fi
  if ! grep -q '❯' "$EVIDENCE_DIR/terminal.txt"; then
    echo "FAIL: terminal.txt missing composer glyph for ${VP}" >&2
    exit 1
  fi
  echo "OK: ${VP}"
done

echo "All 7 viewport captures complete."
