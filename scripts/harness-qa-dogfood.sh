#!/usr/bin/env bash
# Offline agent dogfood channel: deterministic golden_path + gitignored QA evidence.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
Usage: scripts/harness-qa-dogfood.sh [--self-test] [--slug <name>] [--help]

Runs offline deterministic golden_path dogfood from the repo root and writes
reviewable evidence under artifacts/qa-evidence/<YYYYMMDD>-<slug>/.

Options:
  --self-test   Use slug "self-test" (default when no slug is given)
  --slug <name> Evidence directory slug (default: self-test)
  --help        Show this help

Evidence files:
  README.md, commands.log, isolation-receipt.txt, events.jsonl,
  events-excerpt.jsonl, lane-or-run-summary.txt

This smoke is offline; live-provider and terminal evidence have separate lanes.
EOF
}

slug="self-test"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --self-test)
      slug="self-test"
      shift
      ;;
    --slug)
      if [[ $# -lt 2 || -z "${2:-}" || "${2:-}" == --* ]]; then
        printf 'Missing value for --slug\n' >&2
        exit 2
      fi
      slug="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

date_stamp="$(date -u +"%Y%m%d")"
evidence_root="${repo_root}/artifacts/qa-evidence"
evidence_dir="${evidence_root}/${date_stamp}-${slug}"
session_dir="${evidence_dir}/sessions"
events_path="${evidence_dir}/events.jsonl"
commands_log="${evidence_dir}/commands.log"
isolation_receipt="${evidence_dir}/isolation-receipt.txt"
events_excerpt="${evidence_dir}/events-excerpt.jsonl"
secret_scan_path="${evidence_dir}/secret-scan.txt"
summary_path="${evidence_dir}/lane-or-run-summary.txt"
readme_path="${evidence_dir}/README.md"
excerpt_lines=40

mkdir -p "${session_dir}"

: >"${commands_log}"

scan_evidence() {
  local secret_hits scan_exit=0
  secret_hits="$(
    shopt -s dotglob nullglob
    scan_paths=()
    for path in "${evidence_dir}"/*; do
      [[ "${path}" == "${secret_scan_path}" ]] || scan_paths+=("${path}")
    done
    grep -RIohE 'sk-|Bearer |BEGIN PRIVATE KEY' "${scan_paths[@]}" 2>/dev/null
  )" || scan_exit=$?
  case "${scan_exit}" in
    0)
      local match_count
      match_count="$(printf '%s\n' "${secret_hits}" | wc -l)"
      printf 'FAIL\nreason=secret_markers\nmatches=%s\n' "${match_count}" >"${secret_scan_path}"
      printf 'Secret scan failed (fail-closed): %s secret markers found\n' "${match_count}" >&2
      ;;
    1)
      printf 'PASS\npatterns=sk-|Bearer |BEGIN PRIVATE KEY\n' >"${secret_scan_path}"
      return 0
      ;;
    *)
      printf 'FAIL\nreason=scanner_error\n' >"${secret_scan_path}"
      printf 'Secret scan failed (fail-closed): scanner error (exit %s)\n' "${scan_exit}" >&2
      ;;
  esac
  return 1
}

log_cmd() {
  local exit_code="$1"
  shift
  {
    printf '+ %s\n' "$*"
    printf 'exit=%s\n' "${exit_code}"
  } >>"${commands_log}"
}

# Isolation: session-dir must stay under evidence or /tmp; never $HOME/.config/harness.
config_harness_home="${HOME}/.config/harness"
{
  printf 'repo_root=%s\n' "${repo_root}"
  printf 'evidence_dir=%s\n' "${evidence_dir}"
  printf 'session_dir=%s\n' "${session_dir}"
  printf 'config_harness_home=%s\n' "${config_harness_home}"
  printf 'isolation_rule=session-dir must be under evidence_dir or /tmp\n'
  printf 'isolation_rule=must not write into $HOME/.config/harness\n'
} >"${isolation_receipt}"

case "${session_dir}" in
  "${evidence_dir}"/* | /tmp/*)
    printf 'session_dir_ok=true\n' >>"${isolation_receipt}"
    ;;
  *)
    printf 'session_dir_ok=false\n' >>"${isolation_receipt}"
    printf 'Isolation failure: session-dir is not under evidence or /tmp: %s\n' "${session_dir}" >&2
    exit 1
    ;;
esac

if [[ "${session_dir}" == "${config_harness_home}"/* || "${session_dir}" == "${config_harness_home}" ]]; then
  printf 'config_harness_untouched=false\n' >>"${isolation_receipt}"
  printf 'Isolation failure: session-dir points at %s\n' "${config_harness_home}" >&2
  exit 1
fi
printf 'config_harness_untouched=true\n' >>"${isolation_receipt}"

cd "${repo_root}"

run_cmd=(
  cargo run -p harness --
  --session-dir "${session_dir}"
  run
  --scenario golden_path
  --deterministic
  --out "${events_path}"
  --print-run-dir
)

set +e
run_output="$( "${run_cmd[@]}" 2>&1 )"
run_exit=$?
set -e
log_cmd "${run_exit}" "${run_cmd[@]}"
printf '%s\n' "${run_output}" >>"${commands_log}"

if [[ "${run_exit}" -ne 0 ]]; then
  printf 'Dogfood run failed with exit %s\n' "${run_exit}" >&2
  scan_evidence || exit 1
  exit "${run_exit}"
fi

run_dir="$(printf '%s\n' "${run_output}" | awk 'NF { last=$0 } END { if (last) print last }')"
{
  printf 'status=PASS\n'
  printf 'scenario=golden_path\n'
  printf 'deterministic=true\n'
  printf 'events_path=%s\n' "${events_path}"
  printf 'session_dir=%s\n' "${session_dir}"
  printf 'print_run_dir=%s\n' "${run_dir}"
  printf 'cargo_run_exit=%s\n' "${run_exit}"
} >"${summary_path}"

if [[ ! -f "${events_path}" ]]; then
  printf 'Missing events file: %s\n' "${events_path}" >&2
  exit 1
fi

head -n "${excerpt_lines}" "${events_path}" >"${events_excerpt}"

cat >"${readme_path}" <<EOF
# Harness QA dogfood evidence

## WHAT

Offline deterministic \`golden_path\` dogfood via \`scripts/harness-qa-dogfood.sh\`.

## OBSERVED

- cargo run exit: ${run_exit}
- events: ${events_path}
- session-dir: ${session_dir}
- print-run-dir: ${run_dir}

## WHY

Prove the real harness offline mock multi-step path still wires tools/runtime and leaves inspectable events without live providers.

## OMITTED

- Live provider authentication/transport
- PTY and native visual signoff
- Docker isolation

## Non-claims

- **Not live** — mock/deterministic only.
- **Not PTY/native** — no terminal visual evidence.

Evidence root is gitignored (\`artifacts/\`). Do not commit secrets.
EOF

# Secret fail-closed scan over the evidence tree.
scan_evidence

printf 'harness-qa dogfood OK\nevidence_dir=%s\n' "${evidence_dir}"
exit 0
