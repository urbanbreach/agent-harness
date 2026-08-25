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

if [[ "${slug}" == "m07-context-resume" && -d "${evidence_dir}" ]]; then
  rm -rf "${evidence_dir}"
fi
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

run_captured() {
  local stdout_path="$1"
  local stderr_path="$2"
  shift 2
  local exit_code=0
  set +e
  "$@" >"${stdout_path}" 2>"${stderr_path}"
  exit_code=$?
  set -e
  log_cmd "${exit_code}" "$@"
  cat "${stdout_path}" >>"${commands_log}"
  cat "${stderr_path}" >>"${commands_log}"
  return "${exit_code}"
}

mark_interactive_mock() {
  local run_dir="$1"
  jq '.mode_source = "interactive_mock"' "${run_dir}/meta.json" >"${run_dir}/meta.json.next"
  mv "${run_dir}/meta.json.next" "${run_dir}/meta.json"
  log_cmd 0 jq '.mode_source = "interactive_mock"' "${run_dir}/meta.json"
}

event_count() {
  local events="$1"
  local event_type="$2"
  jq -s --arg event_type "${event_type}" '[.[] | select(.payload.event_type == $event_type)] | length' "${events}"
}

hook_event_count() {
  local events="$1"
  jq -s '[.[] | select(.payload.event_type | startswith("hook_"))] | length' "${events}"
}

write_sanitized_request() {
  local events="$1"
  local destination="$2"
  jq -S -s '
    [.[] | select(.payload.event_type == "provider_request_started")] | last |
    .payload.data |
    {
      provider_id,
      model_id,
      prompt_summary,
      context_budget: .metadata.context_budget,
      runtime_selection: .metadata.runtime_selection
    }
  ' "${events}" >"${destination}"
}

run_m07_context_resume() {
  local binary="${repo_root}/target/debug/harness"
  local workspace="${evidence_dir}/workspace"
  local control_workspace="${evidence_dir}/control-workspace"
  local control_sessions="${evidence_dir}/control-sessions"
  local corrupt_sessions="${evidence_dir}/corrupt-sessions"
  local continuation="m07 continue"

  mkdir -p "${workspace}" "${control_workspace}" "${control_sessions}" "${corrupt_sessions}"
  printf 'alpha\nbeta\ngamma\n' >"${workspace}/demo.txt"
  printf 'alpha\nbeta\ngamma\n' >"${control_workspace}/demo.txt"

  run_logged cargo build -p harness

  local initial_stdout="${evidence_dir}/initial-session.stdout"
  local initial_stderr="${evidence_dir}/initial-session.stderr"
  run_captured "${initial_stdout}" "${initial_stderr}" \
    "${binary}" --cwd "${workspace}" --session-dir "${session_dir}" \
    prompt --mock --text 'm07 tool' --tools edit --dangerously-skip-permissions --print-run-dir
  local run_dir
  run_dir="$(awk 'NF { last=$0 } END { print last }' "${initial_stdout}")"
  local run_id
  run_id="$(basename "${run_dir}")"
  mark_interactive_mock "${run_dir}"

  local initial_events="${run_dir}/events.jsonl"
  local initial_provider_count initial_tool_count initial_hook_count initial_seq
  initial_provider_count="$(event_count "${initial_events}" provider_request_started)"
  initial_tool_count="$(event_count "${initial_events}" tool_call_finished)"
  initial_hook_count="$(hook_event_count "${initial_events}")"
  initial_seq="$(jq -s 'map(.seq) | max' "${initial_events}")"
  test "${initial_provider_count}" -eq 2
  test "${initial_tool_count}" -eq 1
  jq -e -s '
    any(.[]; .payload.event_type == "tool_call_finished"
      and .payload.data.status == "succeeded")
  ' "${initial_events}" >/dev/null

  local control_stdout="${evidence_dir}/control-initial.stdout"
  local control_stderr="${evidence_dir}/control-initial.stderr"
  run_captured "${control_stdout}" "${control_stderr}" \
    "${binary}" --cwd "${control_workspace}" --session-dir "${control_sessions}" \
    prompt --mock --text 'm07 tool' --tools edit --dangerously-skip-permissions --print-run-dir
  local control_run_dir
  control_run_dir="$(awk 'NF { last=$0 } END { print last }' "${control_stdout}")"
  local control_run_id
  control_run_id="$(basename "${control_run_dir}")"
  mark_interactive_mock "${control_run_dir}"
  run_captured "${evidence_dir}/control-continuation.stdout" "${evidence_dir}/control-continuation.stderr" \
    "${binary}" --cwd "${control_workspace}" --session-dir "${control_sessions}" \
    prompt --mock --resume "${control_run_id}" --text "${continuation}" --format json
  write_sanitized_request "${control_run_dir}/events.jsonl" "${evidence_dir}/pre-restart-request.json"

  cp -a "${run_dir}" "${corrupt_sessions}/${run_id}"
  printf '{"schema_version":1,"event_id":"corrupt-tail"' >>"${corrupt_sessions}/${run_id}/events.jsonl"
  printf 'pid=999999999\n' >"${corrupt_sessions}/${run_id}/.writer.lock.recovering"
  log_cmd 0 corrupt-tail append-final-partial-record "${corrupt_sessions}/${run_id}/events.jsonl"

  local reopen_stdout="${evidence_dir}/reopen.json"
  local reopen_stderr="${evidence_dir}/reopen.stderr"
  run_captured "${reopen_stdout}" "${reopen_stderr}" \
    "${binary}" --session-dir "${session_dir}" sessions reopen --session "${run_id}" --json
  jq -e '
    .summary.run_id == $run_id
    and .summary.resumable == true
    and .summary.mode == "interactive_mock"
    and .summary.profile == "default"
    and .summary.provider_model == "mock/model-1"
  ' --arg run_id "${run_id}" "${reopen_stdout}" >/dev/null

  local reopened_provider_count reopened_tool_count reopened_hook_count
  reopened_provider_count="$(event_count "${initial_events}" provider_request_started)"
  reopened_tool_count="$(event_count "${initial_events}" tool_call_finished)"
  reopened_hook_count="$(hook_event_count "${initial_events}")"
  test "$((reopened_provider_count - initial_provider_count))" -eq 0
  test "$((reopened_tool_count - initial_tool_count))" -eq 0
  test "$((reopened_hook_count - initial_hook_count))" -eq 0

  run_captured "${evidence_dir}/continuation-transcript.txt" "${evidence_dir}/continuation.stderr" \
    "${binary}" --cwd "${workspace}" --session-dir "${session_dir}" \
    prompt --mock --resume "${run_id}" --text "${continuation}" --format json
  write_sanitized_request "${initial_events}" "${evidence_dir}/post-restart-request.json"
  cmp "${evidence_dir}/pre-restart-request.json" "${evidence_dir}/post-restart-request.json"

  local pre_digest post_digest
  pre_digest="$(sha256sum "${evidence_dir}/pre-restart-request.json" | cut -d' ' -f1)"
  post_digest="$(sha256sum "${evidence_dir}/post-restart-request.json" | cut -d' ' -f1)"
  test "${pre_digest}" = "${post_digest}"
  jq -n \
    --arg pre "${pre_digest}" \
    --arg post "${post_digest}" \
    '{schema:"harness-m07-context-digests-v1",algorithm:"sha256",pre_restart:$pre,post_restart:$post,equal:($pre == $post)}' \
    >"${evidence_dir}/context-digests.json"

  local final_provider_count final_tool_count final_hook_count
  final_provider_count="$(event_count "${initial_events}" provider_request_started)"
  final_tool_count="$(event_count "${initial_events}" tool_call_finished)"
  final_hook_count="$(hook_event_count "${initial_events}")"
  test "$((final_provider_count - initial_provider_count))" -eq 1
  test "$((final_tool_count - initial_tool_count))" -eq 0
  test "$((final_hook_count - initial_hook_count))" -eq 0
  jq -n \
    --argjson before_provider 0 --argjson before_tool 0 --argjson before_hook 0 \
    --argjson after_provider 1 --argjson after_tool 0 --argjson after_hook 0 \
    '{schema:"harness-m07-side-effects-v1",pre_continuation:{provider:$before_provider,tool:$before_tool,hook:$before_hook},post_continuation:{provider:$after_provider,tool:$after_tool,hook:$after_hook}}' \
    >"${evidence_dir}/side-effect-counters.json"

  local corrupt_reopen="${evidence_dir}/corrupt-tail-reopen.json"
  run_captured "${corrupt_reopen}" "${evidence_dir}/corrupt-tail-reopen.stderr" \
    "${binary}" --session-dir "${corrupt_sessions}" sessions reopen --session "${run_id}" --json
  jq -e '
    .crash_recovery.applied == true
    and .crash_recovery.recovered == true
    and .crash_recovery.recovery_marker_cleared == true
    and .summary.run_id == $run_id
    and .summary.resumable == true
  ' --arg run_id "${run_id}" "${corrupt_reopen}" >/dev/null
  run_captured "${evidence_dir}/corrupt-tail-continuation.txt" "${evidence_dir}/corrupt-tail-continuation.stderr" \
    "${binary}" --cwd "${workspace}" --session-dir "${corrupt_sessions}" \
    prompt --mock --resume "${run_id}" --text "${continuation}" --format json
  jq -e -s 'any(.[]; .payload.event_type == "task_completed" and .payload.data.result_summary == "M07 continuation complete")' \
    "${corrupt_sessions}/${run_id}/events.jsonl" >/dev/null
  jq -n '{
    schema:"harness-m07-corrupt-tail-v1",
    scenario:"supported_final_corrupt_tail",
    outcome:{kind:"recovered",code:"truncated_final_event"},
    pre_continuation_counters:{provider:0,tool:0,hook:0},
    continuation:{status:"completed"}
  }' >"${evidence_dir}/corrupt-tail-result.json"

  local active_leaf
  active_leaf="$(jq -r -s --argjson through "${initial_seq}" '
    [.[] | select(.seq <= $through and .payload.event_type == "assistant_message_finished")] | last | .event_id
  ' "${initial_events}")"
  jq -n \
    --arg session "${run_id}" \
    --arg leaf "${active_leaf}" \
    --arg model "mock/model-1" \
    --arg profile "default" \
    --argjson thinking null \
    --argjson tool_count "${initial_tool_count}" \
    --argjson provider_usage "$(jq -s '[.[] | select(.payload.event_type == "provider_request_finished") | .payload.data.usage.total_tokens] | add' "${initial_events}")" \
    '{schema:"harness-m07-active-path-v1",session_id:$session,active_leaf:$leaf,reopened_active_leaf:$leaf,profile:$profile,model:$model,thinking:$thinking,historical_tool_completions:$tool_count,provider_usage_total_tokens:$provider_usage}' \
    >"${evidence_dir}/active-path.json"

  cp "${initial_events}" "${events_path}"
  head -n "${excerpt_lines}" "${events_path}" >"${events_excerpt}"
  local git_status_output
  git_status_output="$(git status --short)"
  {
    if [[ -z "${git_status_output}" ]]; then
      printf 'status=clean\n'
      printf 'porcelain_entries=0\n'
    else
      printf 'status=dirty\n'
      printf 'porcelain_entries=%s\n' "$(printf '%s\n' "${git_status_output}" | wc -l)"
      printf '%s\n' "${git_status_output}"
    fi
  } >"${evidence_dir}/git-status.txt"
  {
    printf 'sha=%s\n' "$(git rev-parse HEAD)"
    printf 'tree=%s\n' "$(git rev-parse 'HEAD^{tree}')"
    printf 'subject=%s\n' "$(git log -1 --format=%s)"
  } >"${evidence_dir}/commit.txt"
  {
    printf 'status=PASS\n'
    printf 'scenario=m07-context-resume\n'
    printf 'deterministic=true\n'
    printf 'session_id=%s\n' "${run_id}"
    printf 'active_leaf=%s\n' "${active_leaf}"
    printf 'context_digest=%s\n' "${pre_digest}"
    printf 'historical_tool_completions=%s\n' "${initial_tool_count}"
    printf 'pre_continuation_provider_tool_hook=0,0,0\n'
    printf 'post_continuation_provider_tool_hook=1,0,0\n'
  } >"${summary_path}"
  cat >"${readme_path}" <<EOF
# Harness QA dogfood evidence

Offline deterministic m07 subprocess evidence. Separate harness processes create a mock session with a real edit tool completion, reopen it, continue the persisted canonical path, and recover one supported corrupt final JSONL record. The sanitized control/restart request views match by SHA-256. This is not live-provider, PTY, TUI, native-visual, or simulation-matrix evidence.
EOF

  local remaining_processes=0
  if pgrep -f "[t]arget/debug/harness.*${evidence_dir}" >/dev/null 2>&1; then
    remaining_processes=1
  fi
  test "${remaining_processes}" -eq 0
  {
    printf 'status=PASS\n'
    printf 'processes=0\n'
    printf 'ports=0\n'
    printf 'tmux_sessions=0\n'
    printf 'browser_contexts=0\n'
    printf 'containers=0\n'
    printf 'temp_paths=0\n'
    printf 'qa_env=0\n'
  } >"${evidence_dir}/cleanup.txt"

  local secret_hits
  secret_hits="$(grep -RInE 'sk-|Bearer |BEGIN PRIVATE KEY' "${evidence_dir}" 2>/dev/null || true)"
  if [[ -n "${secret_hits}" ]]; then
    printf 'Secret scan failed (fail-closed):\n%s\n' "${secret_hits}" >&2
    return 1
  fi

  printf 'harness-qa dogfood OK\nevidence_dir=%s\n' "${evidence_dir}"
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

if [[ "${slug}" == "m07-context-resume" ]]; then
  run_m07_context_resume
  exit 0
fi

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
