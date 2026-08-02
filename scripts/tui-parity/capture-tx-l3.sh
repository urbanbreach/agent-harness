#!/usr/bin/env bash
# Capture transcript-row L3 evidence for the 2 TX scaffold rows with dedicated
# reference freezes:
#   TX-TOOL  pty_helper:live_tool -> harness-tx-live_tool-pinned-v1
#   TX-DIFF  pty_helper:live_diff -> harness-tx-live_diff-pinned-v1
#
# TX-TOOL and TX-DIFF are status=incomplete in the manifest: their reference
# freezes exist and these captures run deterministically, but pixel parity is
# blocked on tool/edit chrome divergence (collapsed reference chips vs the
# ratified structured harness shell cards). This script is dev/lane-ready
# tooling for the promotion wave once the chrome is ratified; it is not wired
# into run_signoff_parity() while the rows are incomplete.
#
# TX-USER and TX-ASSISTANT share the SHELL-COMPLETE capture
# (harness-shell-live_complete-pinned-v1), produced by capture-shell-lifecycle-l3.sh.
#
# All captures are 120x40, deterministic (HARNESS_DETERMINISTIC=1,
# HARNESS_DISABLE_ANIMATIONS=1, HARNESS_SEED=42), and byte-stable across runs.
#
# Output per scenario:
#   <EVIDENCE_BASE>/<capture-dir>/
#     terminal.png  terminal.txt  terminal-ansi.txt  metadata.json
#
# Requirements: Linux, Chrome/Chromium for PNG (Playwright cache or CHROME_BIN).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_BASE="${EVIDENCE_BASE:-artifacts/qa-evidence/20260717-tui-reference-parity/actual}"
FONT_SIZE=15
DWELL_MS="${DWELL_MS:-2500}"
PRE_DWELL_MS="${PRE_DWELL_MS:-400}"
COLS=120
ROWS=40
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

export HARNESS_DETERMINISTIC=1
export HARNESS_DISABLE_ANIMATIONS=1
export HARNESS_SEED=42
export TERM=xterm-256color
export TZ=UTC
export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export FORCE_COLOR=1
unset NO_COLOR || true

# scenario|capture_dir|required_text_1|required_text_2
CAPTURES=(
  "live_tool|harness-tx-live_tool-pinned-v1|run the echo probe|Ran 6 commands · 6 failed"
  "live_diff|harness-tx-live_diff-pinned-v1|edit the probe file|Waiting for response"
)

for ENTRY in "${CAPTURES[@]}"; do
  IFS='|' read -r SCENARIO CAPTURE_DIR REQUIRE_1 REQUIRE_2 <<<"$ENTRY"
  EVIDENCE_DIR="${EVIDENCE_BASE}/${CAPTURE_DIR}"
  mkdir -p "$EVIDENCE_DIR"

  export HARNESS_TUI_PTY_HELPER_SCENARIO="$SCENARIO"
  CMD="$BIN --exact pty_helper_${SCENARIO} --nocapture"

  echo "Capturing ${SCENARIO} L3 -> ${EVIDENCE_DIR} (${COLS}x${ROWS})"
  node scripts/tui-parity/web-terminal-visual-qa.mjs \
    --title "$CAPTURE_DIR" \
    --command "$CMD" \
    --source-label "pty_helper:${SCENARIO}" \
    --cols "$COLS" \
    --rows "$ROWS" \
    --font-size "$FONT_SIZE" \
    --dwell-ms "$DWELL_MS" \
    --pre-dwell-ms "$PRE_DWELL_MS" \
    --chrome-bin "$CHROME_BIN" \
    --evidence-dir "$EVIDENCE_DIR"

  if [[ ! -f "$EVIDENCE_DIR/terminal.png" ]]; then
    echo "FAIL: terminal.png missing for ${SCENARIO}" >&2
    exit 1
  fi
  if ! grep -q "$REQUIRE_1" "$EVIDENCE_DIR/terminal.txt"; then
    echo "FAIL: ${SCENARIO} L3 missing required text: ${REQUIRE_1}" >&2
    exit 1
  fi
  if [[ -n "$REQUIRE_2" ]] && ! grep -q "$REQUIRE_2" "$EVIDENCE_DIR/terminal.txt"; then
    echo "FAIL: ${SCENARIO} L3 missing required text: ${REQUIRE_2}" >&2
    exit 1
  fi
  echo "OK: ${SCENARIO}"
done

echo "All 2 tx captures complete."
