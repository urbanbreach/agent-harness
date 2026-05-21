#!/usr/bin/env bash

set -u -o pipefail

mode=""
artifact_root=""
dry_run=0
harness_bin=""

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
timestamp="$(date -u +"%Y%m%d-%H%M%S")"

pass_count=0
fail_count=0
dry_run_count=0
skip_count=0

usage() {
  cat <<'EOF'
Usage: scripts/test-lanes.sh <mode> [options]
       scripts/test-lanes.sh --help

Modes:
  fast                 Fast deterministic developer lane. No PTY, live, native visual, stress, ignored, or real-network signoff commands.
  integration          Remaining deterministic non-live, non-native-visual, non-PTY-signoff integration commands using explicit Cargo invocations.
  signoff-pty          Deterministic PTY signoff, single-threaded.
  signoff-browser      Browser/media signoff. Requires browser env and optional browser dependencies.
  signoff-live         Live provider signoff. Requires live env and runs live_proxy_preflight first.
  signoff-native       Native visual signoff. Requires native visual env and runs ignored native visual tests single-threaded.
  stress-offline       Delegates to scripts/stress-harness.sh --mode offline.
  stress-live          Requires live env/config and delegates to scripts/stress-harness.sh --mode live.
  all-deterministic    Runs fast, then integration, then signoff-pty only when PTY support checks pass.
  help                 Show this help.

Options:
  --dry-run            Write command/status artifacts and print commands without executing them.
  --artifact-dir <path>  Artifact root. Default: target/test-lanes/<timestamp>
  --harness-bin <path> Reuse an already-built harness binary for stress lanes.
  --help              Show this help.

Artifacts:
  Each mode writes stage artifacts under <artifact-root>/<mode>/stages/<stage>/:
    - command.txt
    - stdout.txt
    - stderr.txt
    - status.txt
    - verification.txt
  The run also writes <artifact-root>/summary.txt and <artifact-root>/env.txt.

Required environment:
  signoff-live and stress-live require:
    HARNESS_LIVE_PROXY=1
    HARNESS_LIVE_PROXY_CONFIG=<path>
    HARNESS_LIVE_PROXY_PROVIDER=<provider>
    HARNESS_LIVE_PROXY_MODEL=<model>
  signoff-native requires:
    HARNESS_NATIVE_VISUAL=1
    DISPLAY=<display>
  signoff-browser requires:
    HARNESS_BROWSER_SIGNOFF=1
    npx on PATH for Playwright-backed skill diagnostics
  all-deterministic PTY support requires:
    cargo on PATH
    crates/harness-testkit/tests/pty_e2e.rs
    crates/harness-tui/tests/pty_e2e.rs
    HARNESS_TEST_LANES_SKIP_PTY not set to 1
  stress-offline and stress-live pass --harness-bin to scripts/stress-harness.sh when
    --harness-bin is supplied or an existing target/debug/harness can be reused.
EOF
}

abspath() {
  local input="$1"
  if [[ "$input" = /* ]]; then
    printf '%s\n' "$input"
  else
    printf '%s/%s\n' "$PWD" "$input"
  fi
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
    --dry-run)
      dry_run=1
      shift
      ;;
    --artifact-dir)
      require_option_value "$1" "${2-}"
      artifact_root="$(abspath "$2")"
      shift 2
      ;;
    --harness-bin)
      require_option_value "$1" "${2-}"
      harness_bin="$(abspath "$2")"
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    fast|integration|signoff-pty|signoff-browser|signoff-live|signoff-native|stress-offline|stress-live|all-deterministic|help)
      if [[ -n "$mode" ]]; then
        printf 'Multiple modes provided: %s and %s\n' "$mode" "$1" >&2
        usage >&2
        exit 2
      fi
      mode="$1"
      shift
      ;;
    *)
      printf 'Unknown argument or mode: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$mode" || "$mode" == "help" ]]; then
  usage
  exit 0
fi

if [[ -z "$artifact_root" ]]; then
  artifact_root="${repo_root}/target/test-lanes/${timestamp}"
fi

summary_path="${artifact_root}/summary.txt"
env_path="${artifact_root}/env.txt"

stress_harness_bin=""
stress_harness_bin_source="none"

resolve_stress_harness_bin() {
  local candidate=""

  if [[ -n "$harness_bin" ]]; then
    if [[ ! -x "$harness_bin" ]]; then
      printf 'Provided --harness-bin is not executable: %s\n' "$harness_bin" >&2
      exit 2
    fi
    stress_harness_bin="$harness_bin"
    stress_harness_bin_source="explicit"
    return 0
  fi

  if [[ -n "${CARGO_BIN_EXE_harness-}" && -x "${CARGO_BIN_EXE_harness}" ]]; then
    stress_harness_bin="${CARGO_BIN_EXE_harness}"
    stress_harness_bin_source="CARGO_BIN_EXE_harness"
    return 0
  fi

  if [[ -n "${CARGO_TARGET_DIR-}" ]]; then
    candidate="$(abspath "$CARGO_TARGET_DIR")/debug/harness"
    if [[ -x "$candidate" ]]; then
      stress_harness_bin="$candidate"
      stress_harness_bin_source="CARGO_TARGET_DIR"
      return 0
    fi
  fi

  candidate="${repo_root}/target/debug/harness"
  if [[ -x "$candidate" ]]; then
    stress_harness_bin="$candidate"
    stress_harness_bin_source="target/debug/harness"
  fi
}

resolve_stress_harness_bin

mkdir -p "$artifact_root"

cat >"$summary_path" <<EOF
Harness test lanes summary
repo_root=$repo_root
mode=$mode
artifact_root=$artifact_root
dry_run=$dry_run
EOF

cat >"$env_path" <<EOF
timestamp_utc=$timestamp
repo_root=$repo_root
artifact_root=$artifact_root
mode=$mode
dry_run=$dry_run
HARNESS_LIVE_PROXY=${HARNESS_LIVE_PROXY-}
HARNESS_LIVE_PROXY_CONFIG=${HARNESS_LIVE_PROXY_CONFIG-}
HARNESS_LIVE_PROXY_PROVIDER=${HARNESS_LIVE_PROXY_PROVIDER-}
HARNESS_LIVE_PROXY_MODEL=${HARNESS_LIVE_PROXY_MODEL-}
HARNESS_LIVE_PROXY_VARIANT=${HARNESS_LIVE_PROXY_VARIANT-}
HARNESS_NATIVE_VISUAL=${HARNESS_NATIVE_VISUAL-}
HARNESS_TEST_LANES_SKIP_PTY=${HARNESS_TEST_LANES_SKIP_PTY-}
DISPLAY=${DISPLAY-}
stress_harness_bin=${stress_harness_bin}
stress_harness_bin_source=${stress_harness_bin_source}
EOF

mode_dir_for() {
  printf '%s/%s\n' "$artifact_root" "$1"
}

stage_dir_for() {
  local mode_name="$1"
  local stage_name="$2"
  printf '%s/stages/%s\n' "$(mode_dir_for "$mode_name")" "$stage_name"
}

write_command_file() {
  local output="$1"
  shift
  : >"$output"
  write_quoted_command_line "$output" "$@"
}

write_quoted_command_line() {
  local output="$1"
  shift
  local arg
  for arg in "$@"; do
    printf '%q ' "$arg" >>"$output"
  done
  printf '\n' >>"$output"
}

print_quoted_command_line() {
  local arg
  for arg in "$@"; do
    printf '%q ' "$arg"
  done
  printf '\n'
}

append_summary() {
  local mode_name="$1"
  local stage_name="$2"
  local result="$3"
  local note="$4"
  printf '%s %s %s %s\n' "$mode_name" "$stage_name" "$result" "$note" >>"$summary_path"
}

record_stage_result() {
  local mode_name="$1"
  local stage_name="$2"
  local result="$3"
  local status_path="$4"
  local verification_path="$5"
  local note="$6"

  printf 'result=%s\n' "$result" >>"$status_path"
  append_summary "$mode_name" "$stage_name" "$result" "$note"

  case "$result" in
    PASS)
      pass_count=$((pass_count + 1))
      ;;
    DRY-RUN)
      dry_run_count=$((dry_run_count + 1))
      ;;
    SKIP)
      skip_count=$((skip_count + 1))
      ;;
    *)
      fail_count=$((fail_count + 1))
      ;;
  esac

  printf 'result=%s\nnote=%s\n' "$result" "$note" >>"$verification_path"
  printf '[%s] %s %s\n' "$mode_name" "$stage_name" "$result"
}

run_stage() {
  local mode_name="$1"
  local stage_name="$2"
  local workdir="$3"
  shift 3

  local stage_dir
  stage_dir="$(stage_dir_for "$mode_name" "$stage_name")"
  mkdir -p "$stage_dir"

  local stdout_path="$stage_dir/stdout.txt"
  local stderr_path="$stage_dir/stderr.txt"
  local status_path="$stage_dir/status.txt"
  local verification_path="$stage_dir/verification.txt"
  local command_path="$stage_dir/command.txt"

  write_command_file "$command_path" "$@"

  if [[ "$dry_run" -eq 1 ]]; then
    printf 'dry-run: ' >"$stdout_path"
    write_quoted_command_line "$stdout_path" "$@"
    printf '[%s] %s command: ' "$mode_name" "$stage_name"
    print_quoted_command_line "$@"
    : >"$stderr_path"
    printf 'command_exit_code=0\ndry_run=true\n' >"$status_path"
    printf 'command_exit_code=0\ndry_run=true\ncommand_not_executed=true\n' >"$verification_path"
    record_stage_result "$mode_name" "$stage_name" DRY-RUN "$status_path" "$verification_path" command_not_executed
    return 0
  fi

  local exit_code
  (
    cd "$workdir" && "$@"
  ) >"$stdout_path" 2>"$stderr_path"
  exit_code=$?

  printf 'command_exit_code=%s\ndry_run=false\n' "$exit_code" >"$status_path"
  printf 'command_exit_code=%s\ndry_run=false\n' "$exit_code" >"$verification_path"

  if [[ "$exit_code" -eq 0 ]]; then
    record_stage_result "$mode_name" "$stage_name" PASS "$status_path" "$verification_path" command_exit_zero
    return 0
  fi

  record_stage_result "$mode_name" "$stage_name" FAIL "$status_path" "$verification_path" "command_exit_${exit_code}"
  return "$exit_code"
}

record_gate_failure() {
  local mode_name="$1"
  local gate_name="$2"
  shift 2

  local stage_dir
  stage_dir="$(stage_dir_for "$mode_name" "$gate_name")"
  mkdir -p "$stage_dir"

  local stdout_path="$stage_dir/stdout.txt"
  local stderr_path="$stage_dir/stderr.txt"
  local status_path="$stage_dir/status.txt"
  local verification_path="$stage_dir/verification.txt"
  local command_path="$stage_dir/command.txt"

  printf 'env-gate %s\n' "$gate_name" >"$command_path"
  : >"$stdout_path"
  printf 'Missing required environment for %s:\n' "$mode_name" >"$stderr_path"
  printf 'command_exit_code=2\ndry_run=false\n' >"$status_path"
  printf 'command_exit_code=2\ndry_run=false\n' >"$verification_path"

  local missing
  for missing in "$@"; do
    printf '  - %s\n' "$missing" | tee -a "$stderr_path" "$verification_path" >&2
  done

  record_stage_result "$mode_name" "$gate_name" FAIL "$status_path" "$verification_path" missing_required_environment
}

record_pty_support_gate() {
  local result="$1"
  local note="$2"
  shift 2

  local mode_name="all-deterministic"
  local gate_name="pty_support"
  local stage_dir
  stage_dir="$(stage_dir_for "$mode_name" "$gate_name")"
  mkdir -p "$stage_dir"

  local stdout_path="$stage_dir/stdout.txt"
  local stderr_path="$stage_dir/stderr.txt"
  local status_path="$stage_dir/status.txt"
  local verification_path="$stage_dir/verification.txt"
  local command_path="$stage_dir/command.txt"

  printf 'pty-support-gate all-deterministic\n' >"$command_path"
  : >"$stderr_path"
  printf 'PTY support gate for all-deterministic\n' >"$stdout_path"
  printf 'command_exit_code=0\ndry_run=%s\n' "$([[ "$dry_run" -eq 1 ]] && printf true || printf false)" >"$status_path"
  printf 'command_exit_code=0\ndry_run=%s\n' "$([[ "$dry_run" -eq 1 ]] && printf true || printf false)" >"$verification_path"

  local detail
  for detail in "$@"; do
    printf '%s\n' "$detail" >>"$stdout_path"
    printf '%s\n' "$detail" >>"$verification_path"
  done

  record_stage_result "$mode_name" "$gate_name" "$result" "$status_path" "$verification_path" "$note"
}

all_deterministic_pty_supported() {
  local unsupported=()

  if [[ "${HARNESS_TEST_LANES_SKIP_PTY-}" == "1" ]]; then
    unsupported+=("HARNESS_TEST_LANES_SKIP_PTY=1")
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    unsupported+=("cargo is not available on PATH")
  fi
  if [[ ! -f "${repo_root}/crates/harness-testkit/tests/pty_e2e.rs" ]]; then
    unsupported+=("missing crates/harness-testkit/tests/pty_e2e.rs")
  fi
  if [[ ! -f "${repo_root}/crates/harness-tui/tests/pty_e2e.rs" ]]; then
    unsupported+=("missing crates/harness-tui/tests/pty_e2e.rs")
  fi

  if [[ "${#unsupported[@]}" -ne 0 ]]; then
    record_pty_support_gate SKIP pty_signoff_unsupported "${unsupported[@]}"
    return 1
  fi

  record_pty_support_gate PASS pty_signoff_supported \
    "cargo available on PATH" \
    "found crates/harness-testkit/tests/pty_e2e.rs" \
    "found crates/harness-tui/tests/pty_e2e.rs" \
    "HARNESS_TEST_LANES_SKIP_PTY is not 1"
  return 0
}

require_live_env() {
  local mode_name="$1"
  if [[ "$dry_run" -eq 1 ]]; then
    return 0
  fi

  local missing=()
  if [[ "${HARNESS_LIVE_PROXY-}" != "1" ]]; then
    missing+=("HARNESS_LIVE_PROXY=1")
  fi
  if [[ -z "${HARNESS_LIVE_PROXY_CONFIG-}" ]]; then
    missing+=("HARNESS_LIVE_PROXY_CONFIG=<path>")
  elif [[ ! -f "${HARNESS_LIVE_PROXY_CONFIG}" ]]; then
    missing+=("HARNESS_LIVE_PROXY_CONFIG points to a readable config file: ${HARNESS_LIVE_PROXY_CONFIG}")
  fi
  if [[ -z "${HARNESS_LIVE_PROXY_PROVIDER-}" ]]; then
    missing+=("HARNESS_LIVE_PROXY_PROVIDER=<provider>")
  fi
  if [[ -z "${HARNESS_LIVE_PROXY_MODEL-}" ]]; then
    missing+=("HARNESS_LIVE_PROXY_MODEL=<model>")
  fi

  if [[ "${#missing[@]}" -ne 0 ]]; then
    record_gate_failure "$mode_name" live_env "${missing[@]}"
    return 1
  fi

  return 0
}

require_native_env() {
  local mode_name="$1"
  if [[ "$dry_run" -eq 1 ]]; then
    return 0
  fi

  local missing=()
  if [[ "${HARNESS_NATIVE_VISUAL-}" != "1" ]]; then
    missing+=("HARNESS_NATIVE_VISUAL=1")
  fi
  if [[ -z "${DISPLAY-}" ]]; then
    missing+=("DISPLAY=<display>")
  fi

  if [[ "${#missing[@]}" -ne 0 ]]; then
    record_gate_failure "$mode_name" native_env "${missing[@]}"
    return 1
  fi

  return 0
}

require_browser_env() {
  local mode_name="$1"
  if [[ "$dry_run" -eq 1 ]]; then
    return 0
  fi

  local missing=()
  if [[ "${HARNESS_BROWSER_SIGNOFF-}" != "1" ]]; then
    missing+=("HARNESS_BROWSER_SIGNOFF=1")
  fi
  if ! command -v npx >/dev/null 2>&1; then
    missing+=("npx on PATH")
  fi

  if [[ "${#missing[@]}" -ne 0 ]]; then
    record_gate_failure "$mode_name" browser_env "${missing[@]}"
    return 1
  fi

  return 0
}

run_fast() {
  run_stage fast fmt "$repo_root" cargo fmt --all -- --check || true
  run_stage fast check "$repo_root" cargo check --workspace || true
  run_stage fast harness_tui_lib "$repo_root" cargo test -p harness-tui --lib || true
  run_stage fast harness_tui_model_switcher_metadata "$repo_root" cargo test -p harness-tui --test model_switcher_metadata || true
  run_stage fast harness_tui_session_navigation_keybindings "$repo_root" cargo test -p harness-tui --test session_navigation_keybindings || true
}

run_integration() {
run_stage integration harness_bootstrap_profiles "$repo_root" cargo test -p harness --test bootstrap_profiles || true
run_stage integration harness_config_docs_reference "$repo_root" cargo test -p harness --test config_docs_reference || true
run_stage integration harness_determinism_multi_turn_tools "$repo_root" cargo test -p harness --test determinism_multi_turn_tools || true
run_stage integration harness_event_docs_reference "$repo_root" cargo test -p harness --test event_docs_reference || true
run_stage integration harness_config_validate "$repo_root" cargo run -p harness -- --config configs/harness.example.jsonc config validate || true
run_stage integration harness_doctor_json "$repo_root" cargo run -p harness -- --config configs/harness.example.jsonc doctor --json || true
run_stage integration harness_testkit_feature_simulator "$repo_root" cargo test -p harness-testkit --test simulator_e2e
run_stage integration harness_doctor_strict_parity "$repo_root" cargo run -p harness -- --config configs/harness.example.jsonc doctor --json --strict-parity
run_stage integration harness_workflow_cli "$repo_root" cargo test -p harness --test workflow_cli || true
run_stage integration harness_core_workflow "$repo_root" cargo test -p harness-core workflow || true
run_stage integration harness_core_architecture_audit "$repo_root" cargo test -p harness-core --test architecture_audit || true
run_stage integration harness_core_replay_golden "$repo_root" cargo test -p harness-core --test replay_golden || true
run_stage integration harness_testkit_workflow_simulator "$repo_root" cargo test -p harness-testkit workflow_simulator || true
run_stage integration harness_forbidden_branding "$repo_root" python3 scripts/check-forbidden-branding.py || true
  run_stage integration harness_prompt_cli "$repo_root" cargo test -p harness --test prompt_cli || true
  run_stage integration harness_replay_sessions_cli "$repo_root" cargo test -p harness --test replay_sessions_cli || true
  run_stage integration harness_run_cli "$repo_root" cargo test -p harness --test run_cli || true
  run_stage integration harness_stress_harness_script "$repo_root" cargo test -p harness --test stress_harness_script || true
  run_stage integration harness_tui_cli_replay_flag_bypasses_launcher_shell "$repo_root" cargo test -p harness --test tui_cli replay_flag_bypasses_launcher_shell || true
  run_stage integration harness_providers_lib "$repo_root" cargo test -p harness-providers --lib || true
  run_stage integration harness_providers_openai_native_schema "$repo_root" cargo test -p harness-providers --test openai_compatible_serializes_native_tool_schema_without_alias_dupes || true
  run_stage integration harness_tools_lib "$repo_root" cargo test -p harness-tools --lib || true
  run_stage integration harness_tools_native_tool_parity_matrix "$repo_root" cargo test -p harness-tools --test native_tool_parity_matrix || true
  run_stage integration harness_tools_hashline_apply "$repo_root" cargo test -p harness-tools --test hashline_apply || true
  run_stage integration harness_tools_mcp_generic "$repo_root" cargo test -p harness-tools --test mcp_generic || true
  run_stage integration harness_tools_native_agent_spawn_child_session_observability "$repo_root" cargo test -p harness-tools --test native_agent_spawn_child_session_observability || true
  run_stage integration harness_tools_native_agent_spawn_batch_lineage "$repo_root" cargo test -p harness-tools --test native_agent_spawn_and_batch_preserve_lineage_permissions_and_order || true
  run_stage integration harness_tools_native_code_lsp "$repo_root" cargo test -p harness-tools --test native_code_lsp || true
  run_stage integration harness_tools_native_code_search "$repo_root" cargo test -p harness-tools --test native_code_search || true
  run_stage integration harness_tools_native_github "$repo_root" cargo test -p harness-tools --test native_github || true
  run_stage integration harness_tools_native_question_tool "$repo_root" cargo test -p harness-tools --test native_question_tool || true
  run_stage integration harness_tools_native_web_fetch "$repo_root" cargo test -p harness-tools --test native_web_fetch || true
  run_stage integration harness_tools_native_web_search "$repo_root" cargo test -p harness-tools --test native_web_search || true
  run_stage integration harness_tools_native_workspace_edit_routing "$repo_root" cargo test -p harness-tools --test native_workspace_edit_routing || true
  run_stage integration harness_tools_single_surface_live "$repo_root" cargo test -p harness-tools --test single_surface_live || true
  run_stage integration harness_tools_skill_load_discovery "$repo_root" cargo test -p harness-tools --test skill_load_discovery || true
  run_stage integration harness_testkit_lib "$repo_root" cargo test -p harness-testkit --lib || true
  run_stage integration harness_testkit_live_proxy_local_helpers "$repo_root" cargo test -p harness-testkit --test live_proxy_e2e || true
  run_stage integration harness_testkit_secretscan "$repo_root" cargo test -p harness-testkit --test secretscan || true
  run_stage integration harness_tools_native_execution_surface "$repo_root" cargo test -p harness-tools --test native_execution_surface || true
  run_stage integration harness_tools_native_control_plane_tools "$repo_root" cargo test -p harness-tools --test native_control_plane_tools || true
  run_stage integration harness_core_deterministic_summary_uses_required_harness_sections "$repo_root" cargo test -p harness-core deterministic_summary_uses_required_harness_sections || true
  run_stage integration harness_core_model_summary_validation_rejects_missing_required_harness_section "$repo_root" cargo test -p harness-core model_summary_validation_rejects_missing_required_harness_section || true
  run_stage integration harness_core_compaction_trigger_pre_prompt_uses_estimate_without_provider_usage "$repo_root" cargo test -p harness-core compaction_trigger_pre_prompt_uses_estimate_without_provider_usage || true
  run_stage integration harness_core_compaction_trigger_uses_fallback_budget_without_model_metadata "$repo_root" cargo test -p harness-core compaction_trigger_uses_fallback_budget_without_model_metadata || true
  run_stage integration harness_core_failed_turn_context "$repo_root" cargo test -p harness-core failed_turn_context || true
  run_stage integration harness_core_failed_terminal_compaction_preserves_original_failure "$repo_root" cargo test -p harness-core failed_terminal_compaction_preserves_original_failure || true
  run_stage integration harness_core_split_oversized_turn "$repo_root" cargo test -p harness-core split_oversized_turn || true
  run_stage integration harness_core_operational_memory "$repo_root" cargo test -p harness-core operational_memory || true
  run_stage integration harness_config_schema_cli_public_runtime_config_accepts_new_compaction_settings "$repo_root" cargo test -p harness --test config_schema_cli public_runtime_config_accepts_new_compaction_settings || true
  run_stage integration harness_config_schema_cli_public_runtime_config_accepts_compaction_settings "$repo_root" cargo test -p harness --test config_schema_cli public_runtime_config_accepts_compaction_settings || true
  run_stage integration harness_core_conversation_projection_failed_checkpoint_turn_status "$repo_root" cargo test -p harness-core conversation_projection_failed_checkpoint_turn_status || true
  run_stage integration harness_core_resume_plan_session_catalog_counts_checkpoint_artifacts_alongside_tool_artifacts "$repo_root" cargo test -p harness-core --test resume_plan session_catalog_counts_checkpoint_artifacts_alongside_tool_artifacts || true
}

run_signoff_pty() {
  run_stage signoff-pty harness_testkit_pty_e2e "$repo_root" env RUST_TEST_THREADS=1 cargo test -p harness-testkit pty_e2e || true
  run_stage signoff-pty harness_tui_pty_e2e "$repo_root" env RUST_TEST_THREADS=1 cargo test -p harness-tui pty_e2e || true
}

run_signoff_browser() {
  require_browser_env signoff-browser || return 0
  run_stage signoff-browser harness_doctor_browser_media "$repo_root" cargo run -p harness -- --config configs/harness.example.jsonc doctor --json || true
  run_stage signoff-browser harness_tools_look_at_media "$repo_root" cargo test -p harness-tools --test native_execution_surface native_look_at_extracts_text_and_routes_media || true
  run_stage signoff-browser harness_tools_terminal_dependency_gate "$repo_root" cargo test -p harness-tools --test native_execution_surface native_terminal_tools_are_registered_and_dependency_gated || true
}

run_signoff_live() {
  require_live_env signoff-live || return 0
  run_stage signoff-live live_proxy_preflight "$repo_root" cargo test -p harness-testkit live_proxy_preflight -- --ignored --exact || true
  run_stage signoff-live live_proxy_prompt_parity_signoff "$repo_root" cargo test -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact || true
  run_stage signoff-live live_proxy_e2e_tui_parity_signoff "$repo_root" cargo test -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact || true
}

run_signoff_native() {
  require_native_env signoff-native || return 0
  run_stage signoff-native native_visual_e2e_ignored "$repo_root" cargo test -p harness-testkit --test native_visual_e2e -- --ignored --test-threads=1 || true
}

run_stress_offline() {
  local stress_dir
  stress_dir="$(mode_dir_for stress-offline)/stress-harness"
  local stress_args=("${script_dir}/stress-harness.sh" --mode offline --artifact-dir "$stress_dir")
  if [[ -n "$stress_harness_bin" ]]; then
    stress_args+=(--harness-bin "$stress_harness_bin")
  fi
  run_stage stress-offline stress_harness_offline "$repo_root" "${stress_args[@]}" || true
}

run_stress_live() {
  require_live_env stress-live || return 0
  local stress_dir
  stress_dir="$(mode_dir_for stress-live)/stress-harness"
  local live_config="${HARNESS_LIVE_PROXY_CONFIG-<HARNESS_LIVE_PROXY_CONFIG>}"
  local stress_args=("${script_dir}/stress-harness.sh" --mode live --config "$live_config" --artifact-dir "$stress_dir")
  if [[ -n "$stress_harness_bin" ]]; then
    stress_args+=(--harness-bin "$stress_harness_bin")
  fi
  run_stage stress-live stress_harness_live "$repo_root" "${stress_args[@]}" || true
}

run_mode() {
  local mode_name="$1"
  mkdir -p "$(mode_dir_for "$mode_name")/stages"
  printf '\n== %s ==\n' "$mode_name"

  case "$mode_name" in
    fast)
      run_fast
      ;;
    integration)
      run_integration
      ;;
    signoff-pty)
      run_signoff_pty
      ;;
    signoff-browser)
      run_signoff_browser
      ;;
    signoff-live)
      run_signoff_live
      ;;
    signoff-native)
      run_signoff_native
      ;;
    stress-offline)
      run_stress_offline
      ;;
    stress-live)
      run_stress_live
      ;;
    all-deterministic)
      run_mode fast
      run_mode integration
      if all_deterministic_pty_supported; then
        run_mode signoff-pty
      else
        printf '[all-deterministic] signoff-pty SKIP\n'
      fi
      ;;
    *)
      printf 'internal error: unsupported mode %s\n' "$mode_name" >&2
      return 2
      ;;
  esac
}

run_mode "$mode"

printf '\nPASS=%s FAIL=%s DRY_RUN=%s SKIP=%s\n' "$pass_count" "$fail_count" "$dry_run_count" "$skip_count" >>"$summary_path"
printf 'artifact_root=%s\nsummary=%s\n' "$artifact_root" "$summary_path"

if [[ "$fail_count" -ne 0 ]]; then
  exit 1
fi

exit 0
