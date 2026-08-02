#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_BASE="${EVIDENCE_BASE:-artifacts/qa-evidence/20260801-grok-chat-shell-parity-final/harness/final-current-v7}"
CHROME_BIN="${CHROME_BIN:-${HOME}/.cache/ms-playwright/chromium-1228/chrome-linux64/chrome}"
if [[ ! -x "$CHROME_BIN" ]]; then
  CHROME_BIN="$(find "${HOME}/.cache/ms-playwright" -maxdepth 3 -type f -name chrome -perm -111 -print 2>/dev/null | sort | tail -1)"
fi
if [[ -z "$CHROME_BIN" || ! -x "$CHROME_BIN" ]]; then
  echo "Chrome/Chromium not found" >&2
  exit 2
fi

cargo test -p harness-tui --test reference_parity_pty_test --no-run
BIN="$(find target/debug/deps -maxdepth 1 -type f -name 'reference_parity_pty_test-*' ! -name '*.d' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)"
if [[ -z "$BIN" || ! -x "$BIN" ]]; then
  echo "reference_parity_pty_test binary not found" >&2
  exit 2
fi

export HARNESS_DETERMINISTIC=1
export HARNESS_SEED=42
export HARNESS_TUI_PTY_HELPER_SCENARIO=live_mixed_transcript
export TERM=xterm-256color
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export FORCE_COLOR=1
unset HARNESS_DISABLE_ANIMATIONS NO_COLOR || true

EVIDENCE_DIR="$EVIDENCE_BASE/transcript/harness-tx-live_mixed_transcript"
CMD="$BIN --exact pty_helper_live_mixed_transcript --nocapture"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "Harness mixed reasoning, prose, and tools" \
  --command "$CMD" \
  --source-label "pty_helper:live_mixed_transcript" \
  --cols 120 \
  --rows 40 \
  --font-size 15 \
  --pre-dwell-ms 400 \
  --phase-origin-ms 100 \
  --frame-ms 100 \
  --frame-ms 200 \
  --frame-ms 300 \
  --frame-ms 400 \
  --frame-ms 600 \
  --frame-ms 800 \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$EVIDENCE_DIR"

grep -q "Inspecting the temporary test file" "$EVIDENCE_DIR/terminal.txt"
grep -q "Created, inspected, and verified the file successfully." "$EVIDENCE_DIR/terminal.txt"
test "$(grep -c '◆ Read mixed.txt' "$EVIDENCE_DIR/terminal.txt")" -eq 2

export HARNESS_TUI_PTY_HELPER_SCENARIO=live_running_tool
EVIDENCE_DIR="$EVIDENCE_BASE/animation/harness-running-tool"
CMD="$BIN --exact pty_helper_live_running_tool --nocapture"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "Harness running tool animation" \
  --command "$CMD" \
  --source-label "pty_helper:live_running_tool" \
  --cols 120 \
  --rows 40 \
  --font-size 15 \
  --pre-dwell-ms 400 \
  --phase-origin-ms 100 \
  --frame-ms 100 \
  --frame-ms 200 \
  --frame-ms 300 \
  --frame-ms 400 \
  --frame-ms 600 \
  --frame-ms 800 \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$EVIDENCE_DIR"

grep -q "run the echo probe" "$EVIDENCE_DIR/terminal.txt"
grep -q "echo tx-tool-output-probe-line" "$EVIDENCE_DIR/terminal.txt"

export HARNESS_DISABLE_ANIMATIONS=1
export HARNESS_TUI_PTY_HELPER_SCENARIO=live_mixed_transcript_done
EVIDENCE_DIR="$EVIDENCE_BASE/transcript/harness-tx-live_mixed_transcript_done"
CMD="$BIN --exact pty_helper_live_mixed_transcript_done --nocapture"
node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "Harness completed mixed reasoning, prose, and tools" \
  --command "$CMD" \
  --source-label "pty_helper:live_mixed_transcript_done" \
  --cols 120 \
  --rows 40 \
  --font-size 15 \
  --pre-dwell-ms 400 \
  --dwell-ms 1200 \
  --chrome-bin "$CHROME_BIN" \
  --evidence-dir "$EVIDENCE_DIR"

grep -q "Thought for" "$EVIDENCE_DIR/terminal.txt"
grep -q "Worked for" "$EVIDENCE_DIR/terminal.txt"
