#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

FAMILIES=(prompt body reasoning generic-tools shell diff subagent permission question failure recovery completion compaction)
VIEWPORTS=(120x40 80x24 60x20 120x40-restored)
INTERACTIONS=(disclosure-collapse disclosure-expand hover focus selection pageup-detach return-to-live resize reduced-motion-replay)
CHECKPOINTS=(settled-all-families disclosure-collapsed disclosure-expanded hover-focus-selection pageup-detached return-to-live viewport-80x24 viewport-60x20 viewport-120x40-restored reduced-motion-replay)
FRAME_ARTIFACTS=(terminal.txt terminal-ansi.txt terminal.png metadata.json)
DUAL_RUNTIME_SCENARIOS=(
  packet3-baseline-stream--wide-120x40
  packet3-baseline-tool--wide-120x40
  packet3-baseline-diff--wide-120x40
  packet3-baseline-permission--default-80x24
  packet3-baseline-question--default-80x24
  packet3-baseline-fail--wide-120x40
  packet3-baseline-recover--wide-120x40
  packet3-baseline-complete--wide-120x40
  packet3-baseline-scroll--wide-120x40
  packet3-baseline-mouse--wide-120x40
  packet3-baseline-resize--minimum-60x20
  packet3-baseline-cjk--default-80x24
  packet3-baseline-cjk--minimum-60x20
)
AUTHORITY_FILE="configs/tui-fidelity-reference-authority.json"
SELF_TEST_DIRECTORY=""

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

require_commands() {
  local command
  for command in cargo node jq sha256sum; do
    command -v "$command" >/dev/null 2>&1 || fail "packet3 capture prerequisite missing: $command"
  done
  [[ -f scripts/tui-parity/web-terminal-visual-qa.mjs ]] || fail "packet3 capture prerequisite missing: web-terminal-visual-qa.mjs"
  [[ -f crates/harness-tui/tests/reference_parity_pty_test.rs ]] || fail "packet3 capture prerequisite missing: reference_parity_pty_test.rs"
  [[ -f "$AUTHORITY_FILE" ]] || fail "packet3 capture prerequisite missing: $AUTHORITY_FILE"
}

validate_capture() {
  local directory="$1"
  local artifact checkpoint
  for artifact in "${FRAME_ARTIFACTS[@]}" cleanup.json source-receipt.json; do
    [[ -s "$directory/$artifact" ]] || fail "packet3 capture incomplete: $artifact missing"
  done
  jq -e '.status == "clean" and (.survivingPids | length) == 0 and (.errors | length) == 0' "$directory/cleanup.json" >/dev/null \
    || fail "packet3 capture incomplete: cleanup receipt not clean"
  for checkpoint in "${CHECKPOINTS[@]}"; do
    for artifact in "${FRAME_ARTIFACTS[@]}"; do
      [[ -s "$directory/checkpoints/$checkpoint/$artifact" ]] \
        || fail "packet3 capture incomplete: checkpoints/$checkpoint/$artifact missing"
    done
  done
}

write_self_test_fixture() {
  local directory="$1"
  local checkpoint artifact
  mkdir -p "$directory"
  for artifact in "${FRAME_ARTIFACTS[@]}"; do
    printf 'fixture\n' > "$directory/$artifact"
  done
  printf '{"status":"clean","survivingPids":[],"errors":[]}\n' > "$directory/cleanup.json"
  printf '{"schemaVersion":"packet3.capture-source.v1"}\n' > "$directory/source-receipt.json"
  for checkpoint in "${CHECKPOINTS[@]}"; do
    mkdir -p "$directory/checkpoints/$checkpoint"
    for artifact in "${FRAME_ARTIFACTS[@]}"; do
      printf 'fixture\n' > "$directory/checkpoints/$checkpoint/$artifact"
    done
  done
}

self_test() {
  require_commands
  SELF_TEST_DIRECTORY="$(mktemp -d)"
  trap 'rm -rf -- "$SELF_TEST_DIRECTORY"' EXIT
  write_self_test_fixture "$SELF_TEST_DIRECTORY"
  validate_capture "$SELF_TEST_DIRECTORY"
  printf 'families: %s\n' "${FAMILIES[*]}"
  printf 'viewports: %s\n' "${VIEWPORTS[*]}"
  printf 'interactions: %s\n' "${INTERACTIONS[*]}"
  printf 'dual-runtime-scenarios: %s\n' "${DUAL_RUNTIME_SCENARIOS[*]}"
  printf 'adapter-invocations: grok harness\n'
  printf 'checkpoints: %s\n' "${CHECKPOINTS[*]}"
  printf 'artifacts: %s cleanup.json source-receipt.json\n' "${FRAME_ARTIFACTS[*]}"
  printf 'cleanup-check: status=clean survivingPids=[] errors=[]\n'
  printf 'packet3 capture self-test PASS\n'
}

self_test_missing_artifact() {
  require_commands
  SELF_TEST_DIRECTORY="$(mktemp -d)"
  trap 'rm -rf -- "$SELF_TEST_DIRECTORY"' EXIT
  write_self_test_fixture "$SELF_TEST_DIRECTORY"
  rm -- "$SELF_TEST_DIRECTORY/terminal-ansi.txt"
  validate_capture "$SELF_TEST_DIRECTORY"
}

case "${1:-}" in
  --self-test) self_test; exit 0 ;;
  --self-test-missing-artifact) self_test_missing_artifact ;;
  "") ;;
  *) fail "usage: $0 [--self-test|--self-test-missing-artifact]" ;;
esac

require_commands
: "${EVIDENCE_DIR:?EVIDENCE_DIR must be explicit}"
: "${REFERENCE_EVIDENCE_DIR:?REFERENCE_EVIDENCE_DIR must be explicit}"
: "${REFERENCE_RECEIPT:?REFERENCE_RECEIPT must be explicit}"
: "${REFERENCE_ROOT:?REFERENCE_ROOT must be explicit}"
: "${HARNESS_BIN:?HARNESS_BIN must be explicit}"
: "${CANDIDATE_RECEIPT:?CANDIDATE_RECEIPT must be explicit}"
: "${CHROME_BIN:?CHROME_BIN must be explicit}"
: "${INDEX_HTML_SHA256:?INDEX_HTML_SHA256 must be explicit}"
[[ -x "$CHROME_BIN" ]] || fail "packet3 capture prerequisite missing: executable CHROME_BIN"
[[ "$(sha256sum index.html | cut -d' ' -f1)" == "$INDEX_HTML_SHA256" ]] \
  || fail "packet3 capture source mismatch: starting index.html hash"

REFERENCE_BIN="$(jq -er '.reference.executable' "$AUTHORITY_FILE")"
REFERENCE_SHA256="$(jq -er '.reference.binary_sha256' "$AUTHORITY_FILE")"
REFERENCE_REVISION="$(jq -er '.reference.source_revision' "$AUTHORITY_FILE")"
[[ -x "$REFERENCE_BIN" ]] || fail "packet3 capture prerequisite missing: authority reference executable"
[[ "$(sha256sum "$REFERENCE_BIN" | cut -d' ' -f1)" == "$REFERENCE_SHA256" ]] \
  || fail "packet3 capture source mismatch: authority reference binary hash"
[[ "$(git -C "$REFERENCE_ROOT" rev-parse HEAD)" == "$REFERENCE_REVISION" ]] \
  || fail "packet3 capture source mismatch: authority reference revision"
[[ -s "$REFERENCE_RECEIPT" && -x "$HARNESS_BIN" && -s "$CANDIDATE_RECEIPT" ]] \
  || fail "packet3 capture prerequisite missing: dual-runtime receipt or candidate binary"
jq -e --arg revision "$REFERENCE_REVISION" --arg digest "$REFERENCE_SHA256" \
  '.schema_version == "harness.tui-fidelity.reference-binary-receipt.v1"
   and .source.revision == $revision and .source.clean == true
   and .binary.sha256 == $digest' "$REFERENCE_RECEIPT" >/dev/null \
  || fail "packet3 capture source mismatch: reference receipt authority binding"

cargo build -p harness-testkit --bin tui-fidelity
RUNNER="${TUI_FIDELITY_BIN:-target/debug/tui-fidelity}"
[[ -x "$RUNNER" ]] || fail "packet3 capture prerequisite missing: tui-fidelity runner"

SUITE_ROOT="$(dirname "$EVIDENCE_DIR")"
mkdir -p "$EVIDENCE_DIR" "$REFERENCE_EVIDENCE_DIR" "$SUITE_ROOT/comparisons"
for scenario in "${DUAL_RUNTIME_SCENARIOS[@]}"; do
  scenario_family="${scenario%%--*}"
  scenario_family="${scenario_family#packet3-}"
  comparison="$SUITE_ROOT/comparisons/$scenario"
  "$RUNNER" compare --scenario "$scenario" --acceptance full-parity \
    --reference-bin "$REFERENCE_BIN" --reference-receipt "$REFERENCE_RECEIPT" \
    --reference-root "$REFERENCE_ROOT" --harness-bin "$HARNESS_BIN" \
    --candidate-receipt "$CANDIDATE_RECEIPT" --evidence-dir "$comparison" \
    --browser-bin "$CHROME_BIN" --timeout-ms 30000
  jq -e '.capture_succeeded == true and .comparison_passed == true' "$comparison/comparison.json" >/dev/null \
    || fail "packet3 capture comparison failed: $scenario"
  jq -e '.status == "clean" and (.surviving_pids | length) == 0 and (.cleanup_errors | length) == 0' "$comparison/cleanup.json" >/dev/null \
    || fail "packet3 capture incomplete: $scenario cleanup receipt not clean"
  cp -a "$comparison/grok" "$REFERENCE_EVIDENCE_DIR/$scenario"
  cp -a "$comparison/harness" "$EVIDENCE_DIR/$scenario"
  for adapter in grok harness; do
    target="$REFERENCE_EVIDENCE_DIR/$scenario"
    [[ "$adapter" == harness ]] && target="$EVIDENCE_DIR/$scenario"
    jq -n --arg adapter "$adapter" --arg scenario "$scenario" \
      --arg revision "$REFERENCE_REVISION" --arg index "$INDEX_HTML_SHA256" \
      '{schemaVersion:"packet3.capture-source.v1",adapter:$adapter,scenario:$scenario,referenceRevision:$revision,indexHtmlSha256:$index}' \
      > "$target/source-receipt.json"
    while IFS= read -r checkpoint; do
      for artifact in "${FRAME_ARTIFACTS[@]}" cells.json; do
        [[ -s "$target/$checkpoint/$artifact" ]] \
          || fail "packet3 capture incomplete: $scenario/$adapter/$checkpoint/$artifact missing"
      done
    done < <(jq -r '.checkpoints[].name' "crates/harness-testkit/src/tui_fidelity_scenarios/baseline/${scenario_family#baseline-}.json")
  done
done

cargo test -p harness-tui --test reference_parity_pty_test --no-run
BIN="$(find target/debug/deps -maxdepth 1 -type f -name 'reference_parity_pty_test-*' ! -name '*.d' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)"
[[ -n "$BIN" && -x "$BIN" ]] || fail "packet3 capture prerequisite missing: fresh reference_parity_pty_test binary"

COMPACTION_DIR="$EVIDENCE_DIR/harness-only-compaction"
mkdir -p "$COMPACTION_DIR"
ACTIONS_FILE="$COMPACTION_DIR/actions.json"
printf '%s\n' '[
  {"waitForText":{"text":"PACKET3_COMPACTION","timeoutMs":12000}},
  {"checkpoint":{"name":"settled-all-families"}},
  {"checkpoint":{"name":"disclosure-collapsed"}},
  {"mouse":{"kind":"click","col":8,"row":6}},
  {"checkpoint":{"name":"disclosure-expanded"}},
  {"mouse":{"kind":"move","col":10,"row":8}},
  {"mouse":{"kind":"drag","from":{"col":10,"row":8},"to":{"col":30,"row":8}}},
  {"checkpoint":{"name":"hover-focus-selection"}},
  {"key":{"key":"PageUp"}},
  {"checkpoint":{"name":"pageup-detached"}},
  {"key":{"key":"PageDown"}},
  {"key":{"key":"PageDown"}},
  {"key":{"key":"PageDown"}},
  {"key":{"key":"PageDown"}},
  {"key":{"key":"PageDown"}},
  {"key":{"key":"PageDown"}},
  {"checkpoint":{"name":"return-to-live"}},
  {"resize":{"cols":80,"rows":24}},
  {"checkpoint":{"name":"viewport-80x24"}},
  {"resize":{"cols":60,"rows":20}},
  {"checkpoint":{"name":"viewport-60x20"}},
  {"resize":{"cols":120,"rows":40}},
  {"checkpoint":{"name":"viewport-120x40-restored"}},
  {"checkpoint":{"name":"reduced-motion-replay"}}
]' > "$ACTIONS_FILE"

export HARNESS_TUI_PTY_HELPER_SCENARIO=live_block_grammar
export HARNESS_DETERMINISTIC=1 HARNESS_DISABLE_ANIMATIONS=1 HARNESS_TUI_REDUCED_MOTION=1 HARNESS_SEED=42
export TERM=xterm-256color TZ=UTC LANG=C.UTF-8 LC_ALL=C.UTF-8 FORCE_COLOR=1
unset NO_COLOR || true

node scripts/tui-parity/web-terminal-visual-qa.mjs \
  --title "packet3-block-grammar-l3" \
  --command "$BIN --exact pty_helper_live_block_grammar --nocapture" \
  --source-label "pty_helper:live_block_grammar" \
  --cols 120 --rows 40 --font-size 15 \
  --actions-file "$ACTIONS_FILE" \
  --dwell-ms 1000 --chrome-bin "$CHROME_BIN" --evidence-dir "$COMPACTION_DIR"

printf '{"schemaVersion":"packet3.capture-source.v1","helperSha256":"%s","indexHtmlSha256":"%s"}\n' \
  "$(sha256sum "$BIN" | cut -d' ' -f1)" "$INDEX_HTML_SHA256" > "$COMPACTION_DIR/source-receipt.json"
validate_capture "$COMPACTION_DIR"
printf '{"schemaVersion":"packet3.suite.v1","referenceComparable":false,"reason":"compaction is Harness-only deterministic owner evidence"}\n' \
  > "$COMPACTION_DIR/comparison-scope.json"
[[ "$(sha256sum index.html | cut -d' ' -f1)" == "$INDEX_HTML_SHA256" ]] \
  || fail "packet3 capture source mismatch: final index.html hash"
printf 'packet3 block grammar L3 dual-runtime suite PASS: %s\n' "$SUITE_ROOT"
