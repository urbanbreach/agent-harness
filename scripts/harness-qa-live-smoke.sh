#!/usr/bin/env bash
# Opt-in live smoke pack: fail-closed without live env; budgeted short prompt with redacted evidence.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
Usage: scripts/harness-qa-live-smoke.sh [--self-test-fail-closed] [--slug <name>] [--help]

Budgeted live smoke pack for WS-L1. Requires live proxy env; exits non-zero when
any required variable is missing (fail-closed — never soft-skips to success).

Required env:
  HARNESS_LIVE_PROXY=1
  HARNESS_LIVE_PROXY_CONFIG   readable harness config path
  HARNESS_LIVE_PROXY_PROVIDER provider id
  HARNESS_LIVE_PROXY_MODEL    model id

Optional env:
  HARNESS_LIVE_PROXY_VARIANT  model variant (passed as --variant when set)
  HARNESS_LIVE_SMOKE_TOOL=1   also run one env-safe tool prompt (not matrix ownership)

Options:
  --self-test-fail-closed  Explicit offline self-test mode (still fails without env)
  --slug <name>            Evidence directory slug (default: live-smoke)
  --help                   Show this help

Evidence (green path):
  artifacts/qa-evidence/<YYYYMMDD>-live-<slug>/
    README.md, commands.log, isolation-receipt.txt, budget-receipt.txt,
    events.jsonl, events-excerpt.jsonl, secret-scan.txt, lane-or-run-summary.txt

Non-claims: not tool behavioral matrix ownership (T5); not freestyle quality;
not multi-provider matrix; not PTY/native.
EOF
}

slug="live-smoke"
self_test_fail_closed=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --self-test-fail-closed)
      self_test_fail_closed=1
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
wall_clock_cap_s=120
excerpt_lines=40
max_turns_budget="1-3"

# --- Fail-closed live env gate (never soft-skip to success) ---
missing=()
if [[ "${HARNESS_LIVE_PROXY-}" != "1" ]]; then
  missing+=("HARNESS_LIVE_PROXY=1")
fi
if [[ -z "${HARNESS_LIVE_PROXY_CONFIG-}" ]]; then
  missing+=("HARNESS_LIVE_PROXY_CONFIG=<readable config path>")
elif [[ ! -f "${HARNESS_LIVE_PROXY_CONFIG}" || ! -r "${HARNESS_LIVE_PROXY_CONFIG}" ]]; then
  missing+=("HARNESS_LIVE_PROXY_CONFIG must be a readable file: ${HARNESS_LIVE_PROXY_CONFIG}")
fi
if [[ -z "${HARNESS_LIVE_PROXY_PROVIDER-}" ]]; then
  missing+=("HARNESS_LIVE_PROXY_PROVIDER=<provider>")
fi
if [[ -z "${HARNESS_LIVE_PROXY_MODEL-}" ]]; then
  missing+=("HARNESS_LIVE_PROXY_MODEL=<model>")
fi

if [[ "${#missing[@]}" -ne 0 ]]; then
  fail_closed_dir="${evidence_root}/${date_stamp}-live-fail-closed"
  mkdir -p "${fail_closed_dir}"
  {
    printf 'status=FAIL_CLOSED\n'
    printf 'mode=live-smoke\n'
    printf 'self_test_fail_closed=%s\n' "${self_test_fail_closed}"
    printf 'reason=missing_or_invalid_live_env\n'
    printf 'missing:\n'
    for item in "${missing[@]}"; do
      printf '  - %s\n' "${item}"
    done
    printf 'rule=never soft-skip to success without live env\n'
  } >"${fail_closed_dir}/fail-closed-receipt.txt"
  printf 'Live smoke fail-closed (missing live env):\n' >&2
  for item in "${missing[@]}"; do
    printf '  - %s\n' "${item}" >&2
  done
  if [[ "${self_test_fail_closed}" -eq 1 ]]; then
    printf 'self-test-fail-closed: exit non-zero as expected without live env\n' >&2
  fi
  printf 'fail_closed_receipt=%s\n' "${fail_closed_dir}/fail-closed-receipt.txt" >&2
  exit 1
fi

# --- Green path (live env present) ---
config_path="${HARNESS_LIVE_PROXY_CONFIG}"
# Resolve relative config paths against repo root for preflight clarity.
if [[ "${config_path}" != /* ]]; then
  config_path="${repo_root}/${config_path}"
fi
if [[ ! -f "${config_path}" || ! -r "${config_path}" ]]; then
  printf 'Preflight failed: config not readable: %s\n' "${config_path}" >&2
  exit 1
fi

provider="${HARNESS_LIVE_PROXY_PROVIDER}"
model="${HARNESS_LIVE_PROXY_MODEL}"
variant="${HARNESS_LIVE_PROXY_VARIANT-}"
model_ref="${provider}/${model}"
tool_smoke="${HARNESS_LIVE_SMOKE_TOOL-}"

evidence_dir="${evidence_root}/${date_stamp}-live-${slug}"
session_dir="${evidence_dir}/sessions"
events_path="${evidence_dir}/events.jsonl"
commands_log="${evidence_dir}/commands.log"
isolation_receipt="${evidence_dir}/isolation-receipt.txt"
budget_receipt="${evidence_dir}/budget-receipt.txt"
events_excerpt="${evidence_dir}/events-excerpt.jsonl"
secret_scan_path="${evidence_dir}/secret-scan.txt"
summary_path="${evidence_dir}/lane-or-run-summary.txt"
readme_path="${evidence_dir}/README.md"

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

# Isolation: session-dir must stay under evidence or /tmp; never $HOME/.config/harness.
config_harness_home="${HOME}/.config/harness"
{
  printf 'repo_root=%s\n' "${repo_root}"
  printf 'evidence_dir=%s\n' "${evidence_dir}"
  printf 'session_dir=%s\n' "${session_dir}"
  printf 'config_path=%s\n' "${config_path}"
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

smoke_prompt='Reply with the single word PONG'
run_cmd=(
  cargo run -p harness --
  --config "${config_path}"
  --session-dir "${session_dir}"
  prompt
  --text "${smoke_prompt}"
  --model "${model_ref}"
  --out "${events_path}"
)
if [[ -n "${variant}" ]]; then
  run_cmd+=(--variant "${variant}")
fi

# Wall-clock hard cap (~120s). Prefer GNU/coreutils timeout when available.
timeout_bin=""
if command -v timeout >/dev/null 2>&1; then
  timeout_bin="timeout"
elif command -v gtimeout >/dev/null 2>&1; then
  timeout_bin="gtimeout"
fi

start_epoch="$(date -u +%s)"
set +e
if [[ -n "${timeout_bin}" ]]; then
  run_output="$( "${timeout_bin}" "${wall_clock_cap_s}" "${run_cmd[@]}" 2>&1 )"
  run_exit=$?
  if [[ "${run_exit}" -eq 124 ]]; then
    run_output="${run_output}"$'\n'"wall_clock_timeout=${wall_clock_cap_s}s"
  fi
else
  run_output="$( "${run_cmd[@]}" 2>&1 )"
  run_exit=$?
fi
set -e
end_epoch="$(date -u +%s)"
wall_clock_s=$((end_epoch - start_epoch))

log_cmd "${run_exit}" "${run_cmd[@]}"
printf '%s\n' "${run_output}" >>"${commands_log}"

{
  printf 'max_turns=%s\n' "${max_turns_budget}"
  printf 'wall_clock_cap_s=%s\n' "${wall_clock_cap_s}"
  printf 'wall_clock_s=%s\n' "${wall_clock_s}"
  printf 'cost=unmetered\n'
  printf 'cost_note=usage not metered by this smoke pack; document unmetered when unknown\n'
  printf 'prompt_size=short_fixed\n'
  printf 'timeout_bin=%s\n' "${timeout_bin:-none}"
  printf 'provider=%s\n' "${provider}"
  printf 'model=%s\n' "${model}"
  printf 'model_ref=%s\n' "${model_ref}"
  if [[ -n "${variant}" ]]; then
    printf 'variant=%s\n' "${variant}"
  fi
} >"${budget_receipt}"

if [[ "${run_exit}" -ne 0 ]]; then
  {
    printf 'status=FAIL\n'
    printf 'scenario=live_smoke_pong\n'
    printf 'cargo_run_exit=%s\n' "${run_exit}"
    printf 'events_path=%s\n' "${events_path}"
    printf 'session_dir=%s\n' "${session_dir}"
    printf 'wall_clock_s=%s\n' "${wall_clock_s}"
  } >"${summary_path}"
  printf 'Live smoke prompt failed with exit %s (wall_clock_s=%s)\n' "${run_exit}" "${wall_clock_s}" >&2
  printf '%s\n' "${run_output}" >&2
  exit "${run_exit}"
fi

if [[ ! -f "${events_path}" ]]; then
  printf 'Missing events file: %s\n' "${events_path}" >&2
  exit 1
fi

head -n "${excerpt_lines}" "${events_path}" >"${events_excerpt}"

# Optional env-safe tool path — not tool behavioral matrix ownership (T5).
tool_status="skipped"
tool_exit=0
if [[ "${tool_smoke}" == "1" ]]; then
  tool_events_path="${evidence_dir}/events-tool.jsonl"
  tool_prompt='Use the read tool on README.md and reply with one short sentence summarizing the first heading. Do not run other tools.'
  tool_cmd=(
    cargo run -p harness --
    --config "${config_path}"
    --session-dir "${session_dir}"
    prompt
    --text "${tool_prompt}"
    --model "${model_ref}"
    --out "${tool_events_path}"
  )
  if [[ -n "${variant}" ]]; then
    tool_cmd+=(--variant "${variant}")
  fi
  set +e
  if [[ -n "${timeout_bin}" ]]; then
    tool_output="$( "${timeout_bin}" "${wall_clock_cap_s}" "${tool_cmd[@]}" 2>&1 )"
    tool_exit=$?
  else
    tool_output="$( "${tool_cmd[@]}" 2>&1 )"
    tool_exit=$?
  fi
  set -e
  log_cmd "${tool_exit}" "${tool_cmd[@]}"
  printf '%s\n' "${tool_output}" >>"${commands_log}"
  if [[ "${tool_exit}" -eq 0 ]]; then
    tool_status="PASS"
  else
    tool_status="FAIL"
    printf 'Optional tool smoke failed with exit %s (not matrix ownership)\n' "${tool_exit}" >&2
    printf '%s\n' "${tool_output}" >&2
    exit "${tool_exit}"
  fi
fi

{
  printf 'status=PASS\n'
  printf 'scenario=live_smoke_pong\n'
  printf 'live=true\n'
  printf 'events_path=%s\n' "${events_path}"
  printf 'session_dir=%s\n' "${session_dir}"
  printf 'config_path=%s\n' "${config_path}"
  printf 'provider=%s\n' "${provider}"
  printf 'model=%s\n' "${model}"
  printf 'model_ref=%s\n' "${model_ref}"
  printf 'cargo_run_exit=%s\n' "${run_exit}"
  printf 'wall_clock_s=%s\n' "${wall_clock_s}"
  printf 'tool_smoke=%s\n' "${tool_status}"
  printf 'tool_smoke_exit=%s\n' "${tool_exit}"
} >"${summary_path}"

cat >"${readme_path}" <<EOF
# Harness QA live smoke evidence

## WHAT

Budgeted live smoke pack via \`scripts/harness-qa-live-smoke.sh\` (WS-L1).

Fixed short prompt against real provider transport under an isolated session-dir.

## OBSERVED

- cargo run exit: ${run_exit}
- provider/model: ${model_ref}
- config: ${config_path}
- events: ${events_path}
- session-dir: ${session_dir}
- wall_clock_s: ${wall_clock_s} (cap ${wall_clock_cap_s}s)
- tool_smoke: ${tool_status}

## WHY

Prove live proxy transport/auth and a fixed short non-tool smoke path leave
inspectable, redacted evidence without claiming broader quality or matrix ownership.

## OMITTED

- Full native tool behavioral matrix (T5)
- Agent freestyle quality evaluation
- Multi-provider matrix coverage
- PTY and native visual signoff
- Simulation matrix admission
- Docker isolation

## Non-claims

- **Not tool behavioral matrix ownership (T5)** — optional tool path (if enabled) is a single env-safe smoke, not matrix coverage.
- **Not freestyle quality** — fixed short prompts only.
- **Not multi-provider matrix** — single configured provider/model tuple.
- **Not PTY/native** — no terminal visual evidence.

Evidence root is gitignored (\`artifacts/\`). Do not commit secrets.
EOF

# Secret fail-closed scan over the evidence tree.
secret_hits="$(
  # shellcheck disable=SC2016
  grep -RInE 'sk-|Bearer |BEGIN PRIVATE KEY' "${evidence_dir}" 2>/dev/null || true
)"
if [[ -n "${secret_hits}" ]]; then
  printf 'FAIL\n%s\n' "${secret_hits}" >"${secret_scan_path}"
  printf 'Secret scan failed (fail-closed):\n%s\n' "${secret_hits}" >&2
  exit 1
fi
printf 'PASS\npatterns=sk-|Bearer |BEGIN PRIVATE KEY\n' >"${secret_scan_path}"

printf 'harness-qa live-smoke OK\nevidence_dir=%s\n' "${evidence_dir}"
exit 0
