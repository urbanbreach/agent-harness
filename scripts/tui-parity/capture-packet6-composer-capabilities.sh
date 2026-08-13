#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

AUTHORITY_FILE="${AUTHORITY_FILE:-configs/tui-fidelity-reference-authority.json}"
REFERENCE_RECEIPT="${REFERENCE_RECEIPT:-configs/tui-fidelity-reference-binary-receipt.json}"
RUNNER="${TUI_FIDELITY_BIN:-target/debug/tui-fidelity}"
DRY_RUN=0

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

validate_authority() {
  local binary revision digest receipt_path actual
  binary="$(jq -er '.reference.executable' "$AUTHORITY_FILE")"
  revision="$(jq -er '.reference.source_revision' "$AUTHORITY_FILE")"
  digest="$(jq -er '.reference.binary_sha256' "$AUTHORITY_FILE")"
  receipt_path="$(jq -er '.reference.receipt_path' "$AUTHORITY_FILE")"
  [[ "$receipt_path" == "$REFERENCE_RECEIPT" ]] || fail "reference receipt is not active authority receipt"
  [[ -x "$binary" ]] || fail "active authority binary is unavailable"
  actual="$(sha256sum "$binary" | cut -d' ' -f1)"
  [[ "$actual" == "$digest" ]] || fail "active authority binary digest mismatch"
  jq -e --arg revision "$revision" --arg digest "$digest" --arg path "$binary" \
    '.schema_version == "harness.tui-fidelity.reference-binary-receipt.v1"
     and .source.revision == $revision and .source.clean == true
     and .binary.sha256 == $digest and .binary.path == $path' \
    "$REFERENCE_RECEIPT" >/dev/null || fail "active reference-only receipt mismatch"
}

emit_plan() {
  local viewport
  for viewport in minimum-60x20 default-80x24 standard-100x30 wide-120x40 extra-wide-140x40; do
    printf 'adapter=grok scenario=packet6-composer--%s\n' "$viewport"
    printf 'adapter=harness scenario=packet6-composer--%s\n' "$viewport"
  done
}

case "${1:-}" in
  --dry-run) DRY_RUN=1 ;;
  --self-test)
    validate_authority
    plan="$(emit_plan)"
    [[ "$(grep -c '^adapter=grok ' <<<"$plan")" -eq 5 ]] || fail "Grok plan count"
    [[ "$(grep -c '^adapter=harness ' <<<"$plan")" -eq 5 ]] || fail "Harness plan count"
    printf '%s\nself-test=PASS\n' "$plan"
    exit 0
    ;;
  "") ;;
  *) fail "usage: $0 [--dry-run|--self-test]" ;;
esac

validate_authority
: "${EVIDENCE_DIR:?EVIDENCE_DIR must be explicit}"
: "${HARNESS_BIN:?HARNESS_BIN must be explicit}"
: "${CANDIDATE_RECEIPT:?CANDIDATE_RECEIPT must be explicit}"
: "${REFERENCE_ROOT:?REFERENCE_ROOT must be explicit}"
: "${CAPABILITY_INPUT:?CAPABILITY_INPUT must be explicit}"
[[ -x "$RUNNER" ]] || fail "tui-fidelity runner unavailable"

if [[ "$DRY_RUN" -eq 1 ]]; then
  emit_plan
  exit 0
fi

digest="$(jq -er '.reference.binary_sha256' "$AUTHORITY_FILE")"
binary="$(jq -er '.reference.executable' "$AUTHORITY_FILE")"
mkdir -p "$EVIDENCE_DIR"
"$RUNNER" packet6-capability --input "$CAPABILITY_INPUT" \
  --evidence-root "$(dirname "$CAPABILITY_INPUT")" \
  --output "$EVIDENCE_DIR/capability-receipt.json" --authority-digest "$digest"
for viewport in minimum-60x20 default-80x24 standard-100x30 wide-120x40 extra-wide-140x40; do
  "$RUNNER" compare --scenario "packet6-composer--$viewport" --acceptance full-parity \
    --reference-bin "$binary" --reference-receipt "$REFERENCE_RECEIPT" \
    --reference-root "$REFERENCE_ROOT" --harness-bin "$HARNESS_BIN" \
    --candidate-receipt "$CANDIDATE_RECEIPT" \
    --evidence-dir "$EVIDENCE_DIR/$viewport"
done
