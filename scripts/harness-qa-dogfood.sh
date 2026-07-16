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

Non-claims: not live; not PTY/native; not simulation matrix ownership.
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
summary_path="${evidence_dir}/lane-or-run-summary.txt"
readme_path="${evidence_dir}/README.md"
excerpt_lines=40

mkdir -p "${session_dir}"

: >"${commands_log}"

log_cmd() {
  local exit_code="$1"
  shift
  {
    printf '+ %s\n' "$*"
    printf 'exit=%s\n' "${exit_code}"
  } >>"${commands_log}"
}

run_logged() {
  local exit_code=0
  set +e
  "$@"
  exit_code=$?
  set -e
  log_cmd "${exit_code}" "$@"
  return "${exit_code}"
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
  printf '%s\n' "${run_output}" >&2
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
- Simulation matrix admission / simulation lane ownership
- Docker isolation

## Non-claims

- **Not live** — mock/deterministic only.
- **Not PTY/native** — no terminal visual evidence.
- **Not simulation matrix ownership** — does not replace matrix/validator lanes.

Evidence root is gitignored (\`artifacts/\`). Do not commit secrets.
EOF

# Secret fail-closed scan over the evidence tree.
secret_hits="$(
  # shellcheck disable=SC2016
  grep -RInE 'sk-|Bearer |BEGIN PRIVATE KEY' "${evidence_dir}" 2>/dev/null || true
)"
if [[ -n "${secret_hits}" ]]; then
  printf 'Secret scan failed (fail-closed):\n%s\n' "${secret_hits}" >&2
  exit 1
fi

printf 'harness-qa dogfood OK\nevidence_dir=%s\n' "${evidence_dir}"
exit 0
