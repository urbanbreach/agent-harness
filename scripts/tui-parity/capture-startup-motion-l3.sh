#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_DIR="${EVIDENCE_DIR:-.omo/evidence/startup-capability-motion/xterm}"
FONT_SIZE=15
FONT_FAMILY="${FONT_FAMILY:-Menlo, \"DejaVu Sans Mono\", \"Noto Sans Mono CJK KR\", monospace}"
CHROME_BIN="${CHROME_BIN:-${HOME}/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome}"

if [[ ! -x "$CHROME_BIN" ]]; then
  if command -v google-chrome >/dev/null 2>&1; then
    CHROME_BIN="$(command -v google-chrome)"
  elif command -v chromium >/dev/null 2>&1; then
    CHROME_BIN="$(command -v chromium)"
  else
    echo "blocked: no Chrome binary for xterm.js capture" >&2
    exit 2
  fi
fi

cargo test -p harness-tui --test reference_parity_pty_test --no-run

BIN="$(
  find target/debug/deps -maxdepth 1 -type f -name 'reference_parity_pty_test-*' ! -name '*.d' -printf '%T@ %p\n' \
    | sort -nr \
    | head -1 \
    | cut -d' ' -f2-
)"
if [[ -z "$BIN" || ! -x "$BIN" ]]; then
  echo "blocked: reference_parity_pty_test binary not found" >&2
  exit 2
fi

export HARNESS_TUI_PTY_HELPER_SCENARIO=type_first_startup
export HARNESS_DETERMINISTIC=1
export HARNESS_SEED=42
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
unset HARNESS_DISABLE_ANIMATIONS
unset NO_COLOR

capture_startup() {
  local name="$1"
  local cols="$2"
  local rows="$3"
  local term="$4"
  local colorterm="$5"
  local no_color="$6"
  local unicode_version="$7"
  local destination="${EVIDENCE_DIR}/${name}"
  local command="${BIN} --exact reference_parity_pty_helper_type_first_startup --nocapture"

  node scripts/tui-parity/web-terminal-visual-qa.mjs \
    --title "startup-motion-${name}" \
    --command "$command" \
    --source-label "pty_helper:type_first_startup+motion-checkpoints" \
    --cols "$cols" \
    --rows "$rows" \
    --font-size "$FONT_SIZE" \
    --font-family "$FONT_FAMILY" \
    --term "$term" \
    --colorterm "$colorterm" \
    --no-color "$no_color" \
    --unicode-version "$unicode_version" \
    --pre-dwell-ms 100 \
    --dwell-ms 100 \
    --action '{"waitForText":{"text":"New worktree","timeoutMs":10000}}' \
    --action '{"checkpoint":{"name":"rest-0ms"}}' \
    --action '{"wait":{"ms":640}}' \
    --action '{"checkpoint":{"name":"mid-640ms"}}' \
    --action '{"wait":{"ms":1360}}' \
    --action '{"checkpoint":{"name":"settled-2000ms"}}' \
    --chrome-bin "$CHROME_BIN" \
    --evidence-dir "$destination"

  test -f "${destination}/terminal.png"
  test -f "${destination}/cleanup.json"
  test -f "${destination}/checkpoints/rest-0ms/terminal.png"
  test -f "${destination}/checkpoints/mid-640ms/terminal.png"
  test -f "${destination}/checkpoints/settled-2000ms/terminal.png"
}

capture_startup truecolor-120x32 120 32 xterm-256color truecolor unset 11
capture_startup ansi256-120x32 120 32 xterm-256color unset unset 11
capture_startup ansi16-120x32 120 32 xterm unset unset 11
capture_startup nocolor-ascii-120x32 120 32 dumb unset 1 6
capture_startup hidden-60x20 60 20 xterm-256color truecolor unset 11
capture_startup small-80x24 80 24 xterm-256color truecolor unset 11
capture_startup full-100x30 100 30 xterm-256color truecolor unset 11
capture_startup full-120x40 120 40 xterm-256color truecolor unset 11
capture_startup full-140x40 140 40 xterm-256color truecolor unset 11

FIRST_KEY_DIR="${EVIDENCE_DIR}/first-key-120x32"
CMD="${BIN} --exact reference_parity_pty_helper_type_first_startup --nocapture"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "startup-motion-first-key-120x32" \
  --command "$CMD" \
  --source-label "pty_helper:type_first_startup+first-key" \
  --cols 120 \
  --rows 32 \
  --font-size "$FONT_SIZE" \
  --font-family "$FONT_FAMILY" \
  --term xterm-256color \
  --colorterm truecolor \
  --no-color unset \
  --unicode-version 11 \
  --pre-dwell-ms 100 \
  --dwell-ms 100 \
  --action '{"waitForText":{"text":"New worktree","timeoutMs":10000}}' \
  --action '{"checkpoint":{"name":"startup"}}' \
  --action '{"input":{"text":"x"}}' \
  --action '{"waitForTextAbsent":{"text":"New worktree","timeoutMs":10000}}' \
  --action '{"checkpoint":{"name":"first-key-draft"}}' \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$FIRST_KEY_DIR"

test -f "${FIRST_KEY_DIR}/cleanup.json"
grep -q 'x' "${FIRST_KEY_DIR}/terminal.txt"
if grep -q 'New worktree' "${FIRST_KEY_DIR}/terminal.txt"; then
  echo "FAIL: first key did not dismiss the startup welcome" >&2
  exit 1
fi
