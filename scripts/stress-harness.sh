#!/usr/bin/env bash

set -u -o pipefail

mode="all"
artifact_root=""
harness_bin=""

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
manifest_path="${repo_root}/Cargo.toml"
config_path="${repo_root}/configs/harness.example.jsonc"
fixture_root="${repo_root}/crates/harness-testkit/fixtures/stress_harness"
timestamp="$(date -u +"%Y%m%d-%H%M%S")"

pass_count=0
fail_count=0

usage() {
  cat <<'EOF'
Usage: scripts/stress-harness.sh [options]

Options:
  --mode <offline|live|all>   Which stages to run. Default: all
  --artifact-dir <path>       Where to write copied fixtures, logs, and summaries
  --config <path>             Harness config to validate and use for live prompt stages
  --harness-bin <path>        Reuse an already-built harness binary instead of building
  --help                      Show this help

The script always preserves artifacts. Each stage writes:
  - command.txt
  - stdout.txt
  - stderr.txt
  - status.txt
  - verification.txt
  - events.jsonl (for prompt/run stages)
EOF
}

abspath() {
  local input="$1"
  if [[ "$input" = /* ]]; then
    printf '%s\n' "$input"
    return 0
  fi

  local parent
  local base
  local dir
  parent="$(dirname -- "$input")"
  base="$(basename -- "$input")"
  dir="$(cd "$parent" 2>/dev/null && pwd)" || return 1
  printf '%s/%s\n' "$dir" "$base"
}

require_option_value() {
  local flag="$1"
  local maybe_value="${2-}"
  if [[ -z "$maybe_value" || "$maybe_value" == --* ]]; then
    printf 'Missing value for %s\n' "$flag" >&2
    exit 2
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      require_option_value "$1" "${2-}"
      mode="$2"
      shift 2
      ;;
    --artifact-dir)
      require_option_value "$1" "${2-}"
      artifact_root="$2"
      shift 2
      ;;
    --config)
      require_option_value "$1" "${2-}"
      config_path="$(abspath "$2")" || {
        printf 'Invalid path for --config: %s\n' "$2" >&2
        exit 2
      }
      shift 2
      ;;
    --harness-bin)
      require_option_value "$1" "${2-}"
      harness_bin="$(abspath "$2")" || {
        printf 'Invalid path for --harness-bin: %s\n' "$2" >&2
        exit 2
      }
      shift 2
      ;;
    --help)
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

case "$mode" in
  offline|live|all) ;;
  *)
    printf 'Invalid --mode: %s\n' "$mode" >&2
    usage >&2
    exit 2
    ;;
esac

if [[ -z "$artifact_root" ]]; then
  artifact_root="${repo_root}/target/harness-stress/run-${timestamp}"
else
  artifact_root="$(abspath "$artifact_root")" || {
    printf 'Invalid path for --artifact-dir: %s\n' "$artifact_root" >&2
    exit 2
  }
fi

mkdir -p "$artifact_root/stages" "$artifact_root/sessions"

workspace_root="${artifact_root}/workspace"
prompt_root="${artifact_root}/prompts"
summary_path="${artifact_root}/summary.txt"
env_path="${artifact_root}/env.txt"

if [[ ! -d "$fixture_root/workspace" || ! -d "$fixture_root/prompts" ]]; then
  printf 'fixture root missing: %s\n' "$fixture_root" >&2
  exit 1
fi

rm -rf "$workspace_root" "$prompt_root"
mkdir -p "$workspace_root" "$prompt_root"
cp -R "$fixture_root/workspace/." "$workspace_root"
cp -R "$fixture_root/prompts/." "$prompt_root"

cat >"$summary_path" <<EOF
Harness stress summary
repo_root=$repo_root
mode=$mode
artifact_root=$artifact_root
config_path=$config_path
EOF

cat >"$env_path" <<EOF
timestamp_utc=$timestamp
repo_root=$repo_root
manifest_path=$manifest_path
artifact_root=$artifact_root
workspace_root=$workspace_root
prompt_root=$prompt_root
mode=$mode
config_path=$config_path
EOF

if [[ -n "$harness_bin" ]]; then
  if [[ ! -x "$harness_bin" ]]; then
    printf 'provided --harness-bin is not executable: %s\n' "$harness_bin" >&2
    exit 1
  fi
  printf 'harness_bin=%s\n' "$harness_bin" >>"$env_path"
fi

stage_dir_for() {
  printf '%s/stages/%s\n' "$artifact_root" "$1"
}

write_command_file() {
  local output="$1"
  shift
  : >"$output"
  local arg
  for arg in "$@"; do
    printf '%q ' "$arg" >>"$output"
  done
  printf '\n' >>"$output"
}

append_summary() {
  local stage="$1"
  local status="$2"
  local note="$3"
  printf '%s %s %s\n' "$stage" "$status" "$note" >>"$summary_path"
}

verify_patterns() {
  local file_path="$1"
  local verification_path="$2"
  shift 2

  if [[ ! -f "$file_path" ]]; then
    printf 'missing verification target: %s\n' "$file_path" >>"$verification_path"
    return 1
  fi

  local pattern
  local missing=0
  for pattern in "$@"; do
    if grep -Fq "$pattern" "$file_path"; then
      printf 'found: %s\n' "$pattern" >>"$verification_path"
    else
      printf 'missing: %s\n' "$pattern" >>"$verification_path"
      missing=1
    fi
  done

  return "$missing"
}

run_stage() {
  local name="$1"
  local workdir="$2"
  local stdin_file="$3"
  shift 3

  local stage_dir
  stage_dir="$(stage_dir_for "$name")"
  mkdir -p "$stage_dir"

  local stdout_path="$stage_dir/stdout.txt"
  local stderr_path="$stage_dir/stderr.txt"
  local status_path="$stage_dir/status.txt"
  local verification_path="$stage_dir/verification.txt"
  local command_path="$stage_dir/command.txt"

  write_command_file "$command_path" "$@"

  local exit_code
  if [[ -n "$stdin_file" ]]; then
    (
      cd "$workdir" && "$@" <"$stdin_file"
    ) >"$stdout_path" 2>"$stderr_path"
    exit_code=$?
  else
    (
      cd "$workdir" && "$@"
    ) >"$stdout_path" 2>"$stderr_path"
    exit_code=$?
  fi

  printf 'command_exit_code=%s\n' "$exit_code" >"$status_path"
  printf 'command_exit_code=%s\n' "$exit_code" >"$verification_path"
  LAST_STAGE_DIR="$stage_dir"
  LAST_STAGE_STATUS_PATH="$status_path"
  LAST_STAGE_VERIFICATION_PATH="$verification_path"
  LAST_STAGE_EXIT_CODE="$exit_code"
}

record_stage_result() {
  local name="$1"
  local ok="$2"
  local note="$3"

  if [[ "$ok" -eq 0 ]]; then
    printf 'result=PASS\n' >>"$LAST_STAGE_STATUS_PATH"
    append_summary "$name" PASS "$note"
    pass_count=$((pass_count + 1))
  else
    printf 'result=FAIL\n' >>"$LAST_STAGE_STATUS_PATH"
    append_summary "$name" FAIL "$note"
    fail_count=$((fail_count + 1))
  fi
}

build_harness_if_needed() {
  if [[ -n "$harness_bin" ]]; then
    return 0
  fi

  run_stage \
    build_harness \
    "$repo_root" \
    "" \
    cargo build -p harness --manifest-path "$manifest_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  fi

  local target_dir
  target_dir="$(cargo metadata --format-version 1 --no-deps --manifest-path "$manifest_path" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')" || {
    printf 'failed to resolve cargo target directory\n' >>"$LAST_STAGE_VERIFICATION_PATH"
    record_stage_result build_harness 1 "build_or_reuse_binary"
    return 1
  }

  harness_bin="${target_dir}/debug/harness"
  if [[ "$stage_ok" -eq 0 && ! -x "$harness_bin" ]]; then
    printf 'missing harness binary after build: %s\n' "$harness_bin" >>"$LAST_STAGE_VERIFICATION_PATH"
    stage_ok=1
  fi

  record_stage_result build_harness "$stage_ok" "build_or_reuse_binary"
  if [[ "$stage_ok" -ne 0 ]]; then
    return 1
  fi

  printf 'harness_bin=%s\n' "$harness_bin" >>"$env_path"
  return 0
}

stage_config_validate() {
  run_stage \
    config_validate \
    "$repo_root" \
    "" \
    "$harness_bin" --config "$config_path" config validate

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$LAST_STAGE_DIR/stdout.txt" "$LAST_STAGE_VERIFICATION_PATH" "config valid:"
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result config_validate "$stage_ok" "config_can_be_loaded"
}

stage_prompt_mock_smoke() {
  local stage_dir
  stage_dir="$(stage_dir_for prompt_mock_smoke)"
  local events_path="$stage_dir/events.jsonl"
  local prompt_file="$prompt_root/00_prompt_mock_smoke.txt"

  run_stage \
    prompt_mock_smoke \
    "$workspace_root" \
    "$prompt_file" \
    "$harness_bin" --session-dir "$artifact_root/sessions" prompt --mock --stdin --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"event_type":"task_completed"' 'Hello world'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result prompt_mock_smoke "$stage_ok" "offline_prompt_path"
}

stage_run_golden_path() {
  local stage_dir
  stage_dir="$(stage_dir_for run_golden_path)"
  local events_path="$stage_dir/events.jsonl"

  run_stage \
    run_golden_path \
    "$repo_root" \
    "" \
    "$harness_bin" --session-dir "$artifact_root/sessions" run --scenario golden_path --deterministic --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"event_type":"tool_call_finished"' '"status":"succeeded"' '"event_type":"run_finished"'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result run_golden_path "$stage_ok" "offline_run_path"
}

stage_run_golden_path_interactive() {
  local stage_dir
  stage_dir="$(stage_dir_for run_golden_path_interactive)"
  mkdir -p "$stage_dir"
  local events_path="$stage_dir/events.jsonl"
  local stdin_path="$stage_dir/allow.txt"
  printf 'allow\n' >"$stdin_path"

  run_stage \
    run_golden_path_interactive \
    "$repo_root" \
    "$stdin_path" \
    "$harness_bin" --session-dir "$artifact_root/sessions" run --scenario golden_path_interactive --deterministic --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"event_type":"permission_requested"' '"event_type":"permission_resolved"' '"event_type":"run_finished"'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result run_golden_path_interactive "$stage_ok" "interactive_permission_path"
}

stage_prompt_list_read() {
  local stage_dir
  stage_dir="$(stage_dir_for prompt_list_read)"
  local events_path="$stage_dir/events.jsonl"
  local prompt_file="$prompt_root/01_list_and_read.txt"

  run_stage \
    prompt_list_read \
    "$workspace_root" \
    "$prompt_file" \
    "$harness_bin" --config "$config_path" --session-dir "$artifact_root/sessions" prompt --stdin --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"tool_id":"list"' '"tool_id":"read"' 'README_MARKER_ALPHA' '"status":"succeeded"'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result prompt_list_read "$stage_ok" "live_list_and_read"
}

stage_prompt_glob_grep() {
  local stage_dir
  stage_dir="$(stage_dir_for prompt_glob_grep)"
  local events_path="$stage_dir/events.jsonl"
  local prompt_file="$prompt_root/02_glob_and_grep.txt"

  run_stage \
    prompt_glob_grep \
    "$workspace_root" \
    "$prompt_file" \
    "$harness_bin" --config "$config_path" --session-dir "$artifact_root/sessions" prompt --stdin --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"tool_id":"glob"' '"tool_id":"grep"' 'docs/reference.md' 'BETA_MARKER'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result prompt_glob_grep "$stage_ok" "live_glob_and_grep"
}

stage_prompt_lsp_rust_best_effort() {
  local stage_dir
  stage_dir="$(stage_dir_for prompt_lsp_rust_best_effort)"
  local events_path="$stage_dir/events.jsonl"
  local prompt_file="$prompt_root/03_lsp_rust_best_effort.txt"

  run_stage \
    prompt_lsp_rust_best_effort \
    "$workspace_root" \
    "$prompt_file" \
    "$harness_bin" --config "$config_path" --session-dir "$artifact_root/sessions" prompt --stdin --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"tool_id":"lsp"' '"operation":"fileDiagnostics"' '"status":"succeeded"' 'diagnosticCount' 'src/broken.rs'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result prompt_lsp_rust_best_effort "$stage_ok" "best_effort_lsp_diagnostics"
}

stage_prompt_lsp_markdown_fail_open() {
  local stage_dir
  stage_dir="$(stage_dir_for prompt_lsp_markdown_fail_open)"
  local events_path="$stage_dir/events.jsonl"
  local prompt_file="$prompt_root/04_lsp_markdown_fail_open.txt"

  run_stage \
    prompt_lsp_markdown_fail_open \
    "$workspace_root" \
    "$prompt_file" \
    "$harness_bin" --config "$config_path" --session-dir "$artifact_root/sessions" prompt --stdin --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"tool_id":"lsp"' '"status":"failed"' 'unsupported lsp language extension: .md' 'notes/unsupported.md'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result prompt_lsp_markdown_fail_open "$stage_ok" "fail_open_unsupported_lsp"
}

stage_prompt_absolute_read() {
  local stage_dir
  stage_dir="$(stage_dir_for prompt_absolute_read)"
  mkdir -p "$stage_dir"
  local events_path="$stage_dir/events.jsonl"
  local prompt_file="$stage_dir/absolute-read.txt"
  local absolute_target="$workspace_root/README.md"

  cat >"$prompt_file" <<EOF
Use read on the absolute path ${absolute_target}.
Summarize the marker you found and mention that the absolute path read worked.
EOF

  run_stage \
    prompt_absolute_read \
    "$workspace_root" \
    "$prompt_file" \
    "$harness_bin" --config "$config_path" --session-dir "$artifact_root/sessions" prompt --stdin --out "$events_path"

  local stage_ok=0
  if [[ "$LAST_STAGE_EXIT_CODE" -ne 0 ]]; then
    stage_ok=1
  else
    verify_patterns "$events_path" "$LAST_STAGE_VERIFICATION_PATH" '"tool_id":"read"' 'README_MARKER_ALPHA' "$absolute_target" '"status":"succeeded"'
    if [[ $? -ne 0 ]]; then
      stage_ok=1
    fi
  fi

  record_stage_result prompt_absolute_read "$stage_ok" "absolute_workspace_read"
}

if ! build_harness_if_needed; then
  printf '\nPASS=%s FAIL=%s\n' "$pass_count" "$fail_count" >>"$summary_path"
  printf 'artifact_root=%s\nsummary=%s\n' "$artifact_root" "$summary_path"
  exit 1
fi

stage_config_validate

if [[ "$mode" == "offline" || "$mode" == "all" ]]; then
  stage_prompt_mock_smoke
  stage_run_golden_path
  stage_run_golden_path_interactive
fi

if [[ "$mode" == "live" || "$mode" == "all" ]]; then
  stage_prompt_list_read
  stage_prompt_glob_grep
  stage_prompt_lsp_rust_best_effort
  stage_prompt_lsp_markdown_fail_open
  stage_prompt_absolute_read
fi

printf '\nPASS=%s FAIL=%s\n' "$pass_count" "$fail_count" >>"$summary_path"
printf 'artifact_root=%s\nsummary=%s\n' "$artifact_root" "$summary_path"

if [[ "$fail_count" -ne 0 ]]; then
  exit 1
fi

exit 0
