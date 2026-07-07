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
  fast                 Parallel deterministic T1-T3 developer lane via cargo nextest profile ci.
  integration          Partitioned deterministic T1-T3 nextest run for CI fan-out checks.
  quality-gates        Static test-suite gates for sleeps, globals, real/live deps, focus, taxonomy, and cassette secrets.
  perf                 T4 performance-budget lane via cargo nextest profile perf.
  coverage             Coverage ratchet lane via scripts/coverage-ratchet.sh.
  simulation           Offline deterministic simulation lane with matrix validation, artifacts, same-seed comparison, and secret scan.
  signoff-binary       Real-process CLI shim smoke, env-gated and ignored by default.
  signoff-pty          Deterministic PTY signoff, single-threaded.
  signoff-live         Live provider signoff. Requires live env and runs live_proxy_preflight_requires_live_env first.
  signoff-native       Native visual signoff. Requires native visual env and runs ignored native visual tests single-threaded.
  stress-offline       Delegates to scripts/stress-harness.sh --mode offline.
  stress-live          Requires live env/config and delegates to scripts/stress-harness.sh --mode live.
  all-deterministic    Runs quality-gates, simulation, fast, integration, then signoff-pty only when PTY support checks pass.
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
  signoff-binary sets HARNESS_BINARY_SMOKE=1 and HARNESS_BINARY_SMOKE_ARTIFACT_DIR for the ignored binary smoke.
  all-deterministic PTY support requires:
    cargo on PATH
    crates/harness-testkit/tests/pty_e2e.rs
    crates/harness-tui/tests/pty_e2e.rs
    HARNESS_TEST_LANES_SKIP_PTY not set to 1
  simulation runs only local deterministic mock-provider harness runs and records artifacts under simulation/stages/simulation_evidence/artifacts.
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
    fast|integration|quality-gates|perf|coverage|simulation|signoff-binary|signoff-pty|signoff-live|signoff-native|stress-offline|stress-live|all-deterministic|help)
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

run_fast() {
  run_stage fast fmt "$repo_root" cargo fmt --all -- --check || true
  run_stage fast clippy "$repo_root" cargo clippy --all-targets --all-features --workspace -- -D warnings || true
  run_stage fast check "$repo_root" cargo check --workspace || true
  run_stage fast nextest_ci "$repo_root" cargo nextest run --profile ci --workspace --all-features || true
}

run_integration() {
  run_stage integration nextest_ci_partition_1 "$repo_root" cargo nextest run --profile ci --workspace --all-features --partition hash:1/2 || true
  run_stage integration nextest_ci_partition_2 "$repo_root" cargo nextest run --profile ci --workspace --all-features --partition hash:2/2 || true
}

run_quality_gates() {
  run_stage quality-gates static_test_suite_gates "$repo_root" python3 scripts/check-test-suite-gates.py || true
  run_stage quality-gates forbidden_branding "$repo_root" python3 scripts/check-forbidden-branding.py || true
}

run_perf() {
  local perf_artifacts_dir
  perf_artifacts_dir="$(stage_dir_for perf nextest_perf)/artifacts"
  mkdir -p "$perf_artifacts_dir"
  run_stage perf nextest_perf "$repo_root" env HARNESS_PERF_ARTIFACT_DIR="$perf_artifacts_dir" cargo nextest run --profile perf --workspace --all-features || true
  run_stage perf perf_artifact_freshness "$repo_root" python3 scripts/check-perf-artifacts.py --artifact-dir "$perf_artifacts_dir" || true
}

run_coverage() {
  run_stage coverage coverage_ratchet "$repo_root" scripts/coverage-ratchet.sh || true
}

run_simulation() {
  local simulation_dir
  simulation_dir="$(mode_dir_for simulation)"
  local simulation_data_dir="${simulation_dir}/data"
  local evidence_artifacts_dir="$(stage_dir_for simulation simulation_evidence)/artifacts"
  local baseline_out="${simulation_data_dir}/baseline.events.jsonl"
  local repeat_out="${simulation_data_dir}/repeat.events.jsonl"
  local baseline_run_dir_file="${simulation_data_dir}/baseline-run-dir.txt"
  local repeat_run_dir_file="${simulation_data_dir}/repeat-run-dir.txt"
  local baseline_replay="${simulation_data_dir}/baseline.replay.json"
  local repeat_replay="${simulation_data_dir}/repeat.replay.json"

  mkdir -p "$simulation_data_dir"

  run_stage simulation matrix_and_validator_tests "$repo_root" cargo nextest run -p harness-testkit --test simulation_validator_test || true

  run_stage simulation baseline_golden_path "$repo_root" cargo run -p harness -- --session-dir "${simulation_data_dir}/sessions-baseline" run --scenario golden_path --deterministic --out "$baseline_out" --print-run-dir || true
  if [[ "$dry_run" -eq 0 && -s "$(stage_dir_for simulation baseline_golden_path)/stdout.txt" ]]; then
    awk 'NF { last=$0 } END { if (last) print last }' "$(stage_dir_for simulation baseline_golden_path)/stdout.txt" >"$baseline_run_dir_file"
  fi

  run_stage simulation repeat_golden_path "$repo_root" cargo run -p harness -- --session-dir "${simulation_data_dir}/sessions-repeat" run --scenario golden_path --deterministic --out "$repeat_out" --print-run-dir || true
  if [[ "$dry_run" -eq 0 && -s "$(stage_dir_for simulation repeat_golden_path)/stdout.txt" ]]; then
    awk 'NF { last=$0 } END { if (last) print last }' "$(stage_dir_for simulation repeat_golden_path)/stdout.txt" >"$repeat_run_dir_file"
  fi

  if [[ "$dry_run" -eq 0 && -s "$baseline_run_dir_file" ]]; then
    run_stage simulation baseline_replay "$repo_root" cargo run -p harness -- replay --session "$(cat "$baseline_run_dir_file")" --json || true
    cp "$(stage_dir_for simulation baseline_replay)/stdout.txt" "$baseline_replay"
  else
    run_stage simulation baseline_replay "$repo_root" cargo run -p harness -- replay --session "<baseline-run-dir>" --json || true
  fi

  if [[ "$dry_run" -eq 0 && -s "$repeat_run_dir_file" ]]; then
    run_stage simulation repeat_replay "$repo_root" cargo run -p harness -- replay --session "$(cat "$repeat_run_dir_file")" --json || true
    cp "$(stage_dir_for simulation repeat_replay)/stdout.txt" "$repeat_replay"
  else
    run_stage simulation repeat_replay "$repo_root" cargo run -p harness -- replay --session "<repeat-run-dir>" --json || true
  fi

  run_stage simulation simulation_evidence "$repo_root" cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root "$evidence_artifacts_dir" --matrix "${repo_root}/docs/simulation-matrix.json" --baseline-events "$baseline_out" --baseline-replay "$baseline_replay" --repeat-events "$repeat_out" --repeat-replay "$repeat_replay" --seed 0 || true

  run_stage simulation simulation_secret_scan "$repo_root" env HARNESS_SECRETS_SCAN_ARTIFACTS=1 HARNESS_SIMULATION_ARTIFACT_DIR="$evidence_artifacts_dir" cargo nextest run -p harness-testkit --test secretscan_test || true
}

run_signoff_binary() {
  local binary_smoke_artifacts_dir
  binary_smoke_artifacts_dir="$(stage_dir_for signoff-binary harness_binary_smoke)/artifacts"
  mkdir -p "$binary_smoke_artifacts_dir"
  run_stage signoff-binary harness_binary_smoke "$repo_root" env HARNESS_BINARY_SMOKE=1 HARNESS_BINARY_SMOKE_ARTIFACT_DIR="$binary_smoke_artifacts_dir" cargo nextest run -p harness --test binary_smoke -- --ignored --exact || true
}

run_signoff_pty() {
  run_stage signoff-pty harness_testkit_pty_e2e "$repo_root" env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1 || true
  run_stage signoff-pty harness_tui_pty_e2e "$repo_root" env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1 || true
  local tui_happy_path_artifacts_dir
  tui_happy_path_artifacts_dir="$(stage_dir_for signoff-pty harness_tui_happy_path_pty)/artifacts"
  mkdir -p "$tui_happy_path_artifacts_dir"
  run_stage signoff-pty harness_tui_happy_path_pty "$repo_root" env RUST_TEST_THREADS=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR="$tui_happy_path_artifacts_dir" cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 -- --ignored --exact scripted_tui_happy_path_records_start_prompt_permission_tool_edit_resume_and_quit || true
}

run_signoff_live() {
  require_live_env signoff-live || return 0
  run_stage signoff-live live_proxy_preflight_requires_live_env "$repo_root" cargo nextest run -p harness-testkit live_proxy_preflight_requires_live_env -- --ignored --exact || true
  run_stage signoff-live live_proxy_prompt_parity_signoff "$repo_root" cargo nextest run -p harness-testkit live_proxy_prompt_parity_signoff -- --ignored --exact || true
  run_stage signoff-live live_proxy_e2e_tui_parity_signoff "$repo_root" cargo nextest run -p harness-testkit live_proxy_e2e_tui_parity_signoff -- --ignored --exact || true
}

run_signoff_native() {
  require_native_env signoff-native || return 0
  run_stage signoff-native native_visual_e2e_ignored "$repo_root" cargo nextest run -p harness-testkit --test native_visual_e2e --test-threads 1 -- --ignored || true
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
    quality-gates)
      run_quality_gates
      ;;
    perf)
      run_perf
      ;;
    coverage)
      run_coverage
      ;;
    simulation)
      run_simulation
      ;;
    signoff-binary)
      run_signoff_binary
      ;;
    signoff-pty)
      run_signoff_pty
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
      run_mode quality-gates
      run_mode simulation
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
