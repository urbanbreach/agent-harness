#!/usr/bin/env bash
# Orchestrate clean-room parity captures across every P0-P9 proof-dimension family.
#
# Proof dimensions are defined by crates/harness-testkit/src/parity/status.rs
# (ProofDimension) with per-row-kind applicability (applicable_dimensions_for):
#   terminal_capability -> P0,P1,P2,P3
#   visual              -> P0-P9 (all)
#
# Each family reuses an existing pinned capture script, redirects its evidence
# into this run's evidence base, and records its proof_dimensions in the capture
# metadata.json (via web-terminal-visual-qa.mjs --proof-dimensions). Across all
# families the union of declared dimensions covers P0-P9.
#
# Output:
#   <EVIDENCE_BASE>/terminal-capability/harness-resp-<W>x<H>-pinned-v2/...
#   <EVIDENCE_BASE>/visual/<scenario>/{terminal.png,terminal.txt,terminal-ansi.txt,metadata.json}
#   <EVIDENCE_BASE>/coverage.json   (family -> dimensions rollup; complete=true iff union == P0-P9)
#
# Usage:
#   bash scripts/tui-parity/capture-p0-p9-dimensions.sh --dry-run   # validate wiring, skip cargo/Chrome
#   bash scripts/tui-parity/capture-p0-p9-dimensions.sh             # run every family capture
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

EVIDENCE_BASE="${EVIDENCE_BASE:-.omo/evidence/grok-build-clean-room-parity/20260727-184958/task-5-grok-build-clean-room-parity}"
ALL_DIMENSIONS=("P0" "P1" "P2" "P3" "P4" "P5" "P6" "P7" "P8" "P9")
ALL_DIMENSIONS_CSV="P0,P1,P2,P3,P4,P5,P6,P7,P8,P9"
DRY_RUN=0

usage() {
  cat <<'EOF'
capture-p0-p9-dimensions.sh

Run clean-room parity captures for each proof-dimension family and roll up
coverage across P0-P9 (see crates/harness-testkit/src/parity/status.rs).

Usage:
  bash scripts/tui-parity/capture-p0-p9-dimensions.sh --dry-run
  bash scripts/tui-parity/capture-p0-p9-dimensions.sh

Options:
  --dry-run      Validate family wiring (scripts exist, dimensions canonical) and
                  write coverage.json with per-family status "dry-run"; skip the
                  cargo build and Chrome captures. HARNESS_BIN is NOT required.
  -h, --help     Show this help.

Environment:
  HARNESS_BIN     Absolute path to the candidate harness binary. REQUIRED for real
                  captures (fail-closed). Propagated to every family script so
                  runner_identity in metadata.json records the real candidate.
  EVIDENCE_BASE   Output root. Default: the task-5 clean-room parity evidence dir.
  CHROME_BIN      Chrome/Chromium executable forwarded to each capture script.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

# Fail-closed runner-identity gate: real captures require an absolute HARNESS_BIN.
# Dry-run is a wiring check only and must NOT satisfy freshness.
if [[ $DRY_RUN -eq 0 ]]; then
  if [[ -z "${HARNESS_BIN:-}" ]]; then
    echo "FAIL: HARNESS_BIN is required for real captures (runner-identity fail-closed)" >&2
    echo "      set HARNESS_BIN to the absolute path of the candidate harness binary" >&2
    exit 2
  fi
  if [[ "${HARNESS_BIN:0:1}" != "/" ]]; then
    echo "FAIL: HARNESS_BIN must be an absolute path (got ${HARNESS_BIN})" >&2
    exit 2
  fi
  if [[ ! -x "$HARNESS_BIN" ]]; then
    echo "FAIL: HARNESS_BIN is not executable: ${HARNESS_BIN}" >&2
    exit 2
  fi
  export HARNESS_BIN
fi

# True when every token in a comma-separated list is a canonical P0-P9 dimension.
dims_are_valid() {
  local csv="$1"
  [[ -n "$csv" ]] || return 1
  local IFS=','
  local token
  for token in $csv; do
    token="${token//[[:space:]]/}"
    [[ "$token" =~ ^P[0-9]$ ]] || return 1
    [[ "${ALL_DIMENSIONS_CSV}" == *"$token"* ]] || return 1
  done
  return 0
}

# "P0,P1,P2" -> ["P0","P1","P2"] (tokens are pre-validated, so no escaping needed).
json_dims() {
  local csv="$1" out="[" first=1 IFS=',' token
  for token in $csv; do
    [[ $first -eq 1 ]] || out+=","
    out+="\"${token//[[:space:]]/}\""
    first=0
  done
  printf '%s]' "$out"
}

# Family table: name | row_kind | proof_dimensions_csv | capture_script | output_env_var | subdir
FAMILIES=(
  "terminal-capability|terminal_capability|P0,P1,P2,P3|scripts/tui-parity/capture-resp-idle-shell-l3.sh|EVIDENCE_BASE|terminal-capability"
  "visual-shell-scroll|visual|${ALL_DIMENSIONS_CSV}|scripts/tui-parity/capture-shell-scroll-l3.sh|EVIDENCE_DIR|visual/shell-scroll"
  "visual-question-stream|visual|${ALL_DIMENSIONS_CSV}|scripts/tui-parity/capture-question-stream-l3.sh|EVIDENCE_DIR|visual/question-stream"
  "visual-perm-empty-draft|visual|${ALL_DIMENSIONS_CSV}|scripts/tui-parity/capture-perm-empty-draft-l3.sh|EVIDENCE_DIR|visual/perm-empty-draft"
)

if [[ -n "${CHROME_BIN:-}" ]]; then
  export CHROME_BIN
fi

mkdir -p "$EVIDENCE_BASE"

FAMILIES_JSON=""
FAILED=0

for entry in "${FAMILIES[@]}"; do
  IFS='|' read -r name kind dims script outvar subdir <<<"$entry"
  out_dir="${EVIDENCE_BASE}/${subdir}"

  if ! dims_are_valid "$dims"; then
    echo "FAIL: family '${name}' declares non-canonical proof dimensions: ${dims}" >&2
    exit 1
  fi
  if [[ ! -f "$script" ]]; then
    echo "FAIL: family '${name}' references missing capture script: ${script}" >&2
    exit 1
  fi

  if [[ $DRY_RUN -eq 1 ]]; then
    status="dry-run"
    mkdir -p "$out_dir"
    echo "[dry-run] ${name} (${kind}) dims=${dims}"
    echo "          script=${script}"
    echo "          ${outvar}=${out_dir}"
  else
    echo "Capturing family '${name}' (${kind}) dims=${dims} -> ${out_dir}"
    status="captured"
    if ! env "${outvar}=${out_dir}" bash "$script"; then
      echo "FAIL: family '${name}' capture failed" >&2
      status="failed"
      FAILED=1
    fi
  fi

  family_json=$(printf '{"name":"%s","row_kind":"%s","proof_dimensions":%s,"capture_script":"%s","output_env_var":"%s","evidence_dir":"%s","status":"%s"}' \
    "$name" "$kind" "$(json_dims "$dims")" "$script" "$outvar" "$out_dir" "$status")
  if [[ -z "$FAMILIES_JSON" ]]; then
    FAMILIES_JSON="${family_json}"
  else
    FAMILIES_JSON="${FAMILIES_JSON},${family_json}"
  fi
done

COVERAGE_PATH="${EVIDENCE_BASE}/coverage.json"
FAMILIES_JSON="[${FAMILIES_JSON}]" \
ALL_DIMENSIONS_CSV="${ALL_DIMENSIONS_CSV}" \
COVERAGE_PATH="${COVERAGE_PATH}" \
DRY_RUN="${DRY_RUN}" \
node --input-type=module -e "$(cat <<'NODE'
import { writeFileSync } from "node:fs";

const families = JSON.parse(process.env.FAMILIES_JSON);
const all = process.env.ALL_DIMENSIONS_CSV.split(",").filter(Boolean);
const rank = (d) => Number.parseInt(d.slice(1), 10);
const coveredSet = new Set();
for (const family of families) for (const dim of family.proof_dimensions) coveredSet.add(dim);
const covered = all.filter((d) => coveredSet.has(d)).sort((a, b) => rank(a) - rank(b));
const missing = all.filter((d) => !coveredSet.has(d)).sort((a, b) => rank(a) - rank(b));
const doc = {
  schema: "agent-harness.clean-room-parity.dimension-coverage/v1",
  generated_by: "scripts/tui-parity/capture-p0-p9-dimensions.sh",
  dry_run: process.env.DRY_RUN === "1",
  all_dimensions: all,
  covered_dimensions: covered,
  missing_dimensions: missing,
  complete: missing.length === 0,
  families,
};
writeFileSync(process.env.COVERAGE_PATH, `${JSON.stringify(doc, null, 2)}\n`, "utf8");
process.stdout.write(`coverage: ${covered.length}/${all.length} dimensions${missing.length ? ` (missing: ${missing.join(",")})` : ""} -> ${process.env.COVERAGE_PATH}\n`);
NODE
)"

if [[ "${FAILED}" -ne 0 ]]; then
  echo "FAIL: one or more families failed; see ${COVERAGE_PATH}" >&2
  exit 1
fi
echo "OK: P0-P9 dimension coverage rollup at ${COVERAGE_PATH}"
