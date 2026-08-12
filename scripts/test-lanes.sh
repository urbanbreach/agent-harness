#!/usr/bin/env bash

set -u -o pipefail

mode=""
artifact_root=""
dry_run=0
harness_bin=""
reference_bin=""
reference_receipt=""
reference_root=""

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
  signoff-pty          Strict fail-closed deterministic PTY signoff (dual-binary CLI journeys), single-threaded.
  signoff-live         Live provider signoff. Requires live env and runs live_proxy_preflight_requires_live_env first.
  signoff-native       Native visual signoff. Requires native visual env and runs ignored native visual tests single-threaded.
  signoff-parity       Strict fail-closed dual-binary TUI reference parity (cells/pixels) with executable evidence provenance. Missing manifest/env/binary/owners = FAIL.
  signoff-packet2      Five sequential real-PTY Packet 2 scheduling comparisons against pinned reference assets.
  signoff-journeys     Strict fail-closed A-JOURNEYS scaffolding: offline config CLI journeys + worktree owner doc. Missing binary/owners = FAIL.
  stress-offline       Delegates to scripts/stress-harness.sh --mode offline.
  stress-live          Requires live env/config and delegates to scripts/stress-harness.sh --mode live.
  all-deterministic    Runs quality-gates, simulation, fast, integration, then signoff-pty only when PTY support checks pass.
  help                 Show this help.

Options:
  --dry-run            Write command/status artifacts and print commands without executing them.
  --artifact-dir <path>  Artifact root. Default: target/test-lanes/<timestamp>
  --harness-bin <path> Reuse an already-built harness binary for stress lanes.
  --reference-bin <path> Absolute pinned reference executable for signoff-packet2.
  --reference-receipt <path> Absolute pinned reference receipt for signoff-packet2.
  --reference-root <path> Absolute clean pinned reference worktree for signoff-packet2.
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
  signoff-parity requires (fail-closed; no silent skip):
    docs/reference/tui-reference-parity-manifest.v1.json
    cargo on PATH
    harness-tui owner stages: manifest, p0/shell topology, cells, pixels, first-slice,
      perm/question, tx/shell, responsive, and PTY owners with HARNESS_TUI_PTY_SIGNOFF=1
    This lane owns dual-binary cells/pixels/PTY acceptance; docs/testing/tui-signoff-manifest.v1.json does not.
  signoff-binary sets HARNESS_BINARY_SMOKE=1 and HARNESS_BINARY_SMOKE_ARTIFACT_DIR for the ignored binary smoke.
  signoff-journeys requires (fail-closed; no silent skip):
    crates/harness/tests/journey_signoff_test.rs
    cargo on PATH
    compiled harness binary via cargo nextest (CARGO_BIN_EXE_harness)
    Offline owners: config show --effective, config sources, config explain
    Worktree owner is documented only; full PTY remains HARNESS_TUI_PTY_SIGNOFF=1
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
    --reference-bin)
      require_option_value "$1" "${2-}"
      reference_bin="$2"
      shift 2
      ;;
    --reference-receipt)
      require_option_value "$1" "${2-}"
      reference_receipt="$2"
      shift 2
      ;;
    --reference-root)
      require_option_value "$1" "${2-}"
      reference_root="$2"
      shift 2
      ;;
    --help)
      usage
      exit 0
      ;;
    fast|integration|quality-gates|perf|coverage|simulation|signoff-binary|signoff-pty|signoff-live|signoff-native|signoff-parity|signoff-packet2|signoff-journeys|stress-offline|stress-live|all-deterministic|help)
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

  run_stage simulation simulation_evidence "$repo_root" cargo run -p harness-testkit --bin simulation_evidence -- --artifact-root "$evidence_artifacts_dir" --matrix "${repo_root}/docs/testing/simulation-matrix.json" --baseline-events "$baseline_out" --baseline-replay "$baseline_replay" --repeat-events "$repeat_out" --repeat-replay "$repeat_replay" --seed 0 || true

  run_stage simulation simulation_secret_scan "$repo_root" env HARNESS_SECRETS_SCAN_ARTIFACTS=1 HARNESS_SIMULATION_ARTIFACT_DIR="$evidence_artifacts_dir" cargo nextest run -p harness-testkit --test secretscan_test || true
}

run_signoff_binary() {
  local binary_smoke_artifacts_dir
  binary_smoke_artifacts_dir="$(stage_dir_for signoff-binary harness_binary_smoke)/artifacts"
  mkdir -p "$binary_smoke_artifacts_dir"
  run_stage signoff-binary harness_binary_smoke "$repo_root" env HARNESS_BINARY_SMOKE=1 HARNESS_BINARY_SMOKE_ARTIFACT_DIR="$binary_smoke_artifacts_dir" cargo nextest run -p harness --test binary_smoke --ignore-default-filter -- --ignored --exact || true
}

run_signoff_pty() {
  local mode_name="signoff-pty"
  local dual_binary_artifacts_dir
  dual_binary_artifacts_dir="$(stage_dir_for signoff-pty harness_tui_dual_binary_cli_pty)/artifacts"
  mkdir -p "$dual_binary_artifacts_dir"
  local tui_happy_path_artifacts_dir
  tui_happy_path_artifacts_dir="$(stage_dir_for signoff-pty harness_tui_happy_path_pty)/artifacts"
  mkdir -p "$tui_happy_path_artifacts_dir"

  if [[ "$dry_run" -eq 0 ]]; then
    local missing=()
    if [[ ! -f "${repo_root}/crates/harness-testkit/tests/pty_e2e.rs" ]]; then
      missing+=("missing owner crates/harness-testkit/tests/pty_e2e.rs (silent skip is forbidden)")
    fi
    if [[ ! -f "${repo_root}/crates/harness-tui/tests/pty_e2e.rs" ]]; then
      missing+=("missing owner crates/harness-tui/tests/pty_e2e.rs (silent skip is forbidden)")
    fi
    if [[ ! -f "${repo_root}/crates/harness/tests/pty_happy_path_recorded.rs" ]]; then
      missing+=("missing owner crates/harness/tests/pty_happy_path_recorded.rs (silent skip is forbidden)")
    fi
    if ! command -v cargo >/dev/null 2>&1; then
      missing+=("cargo is not available on PATH")
    fi
    if [[ "${#missing[@]}" -ne 0 ]]; then
      record_gate_failure "$mode_name" pty_prerequisites "${missing[@]}"
      {
        printf 'lane=signoff-pty\n'
        printf 'result=FAIL\n'
        printf 'reason=missing_prerequisites\n'
        printf 'stages=prerequisites,testkit_pty,tui_pty,happy_path,dual_binary\n'
        printf 'owns=deterministic_pty_and_dual_binary_cli_journeys\n'
      } >"${dual_binary_artifacts_dir}/pty-lane-verdict.txt"
      return 1
    fi
  fi

  run_stage "$mode_name" harness_testkit_pty_e2e "$repo_root" \
    env RUST_TEST_THREADS=1 cargo nextest run -p harness-testkit --test pty_e2e --test-threads 1 --ignore-default-filter
  run_stage "$mode_name" harness_tui_pty_e2e "$repo_root" \
    env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 cargo nextest run -p harness-tui --test pty_e2e --test-threads 1 --ignore-default-filter
  run_stage "$mode_name" harness_tui_happy_path_pty "$repo_root" \
    env RUST_TEST_THREADS=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR="$tui_happy_path_artifacts_dir" \
    cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 -- --ignored --exact scripted_tui_happy_path_records_start_prompt_permission_tool_edit_resume_and_quit
  run_stage "$mode_name" harness_tui_dual_binary_cli_pty "$repo_root" \
    env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_TUI_PARITY_STRICT=1 HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR="$dual_binary_artifacts_dir" \
    cargo nextest run -p harness --test pty_happy_path_recorded --test-threads 1 -- --ignored dual_binary_cli_pty

  local mode_failed=0
  local stage_status
  for stage_status in "$(mode_dir_for signoff-pty)"/stages/*/status.txt; do
    if [[ -f "$stage_status" ]] && grep -q '^result=FAIL$' "$stage_status"; then
      mode_failed=1
      break
    fi
  done
  if [[ "$mode_failed" -eq 0 ]]; then
    {
      printf 'lane=signoff-pty\n'
      printf 'result=PASS\n'
      printf 'reason=owners_green\n'
      printf 'stages=testkit_pty,tui_pty,happy_path,dual_binary\n'
      printf 'owns=deterministic_pty_and_dual_binary_cli_journeys\n'
    } >"${dual_binary_artifacts_dir}/pty-lane-verdict.txt"
  else
    {
      printf 'lane=signoff-pty\n'
      printf 'result=FAIL\n'
      printf 'reason=stage_failure\n'
      printf 'stages=testkit_pty,tui_pty,happy_path,dual_binary\n'
      printf 'owns=deterministic_pty_and_dual_binary_cli_journeys\n'
    } >"${dual_binary_artifacts_dir}/pty-lane-verdict.txt"
    return 1
  fi
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

run_signoff_journeys() {
  local mode_name="signoff-journeys"
  local journey_test_rel="crates/harness/tests/journey_signoff_test.rs"
  local journey_test_path="${repo_root}/${journey_test_rel}"
  local journey_artifacts_dir
  journey_artifacts_dir="$(stage_dir_for signoff-journeys journey_evidence)/artifacts"
  mkdir -p "$journey_artifacts_dir"

  if [[ "$dry_run" -eq 0 ]]; then
    local missing=()
    if [[ ! -f "$journey_test_path" ]]; then
      missing+=("missing owner ${journey_test_rel} (silent skip is forbidden)")
    fi
    if ! command -v cargo >/dev/null 2>&1; then
      missing+=("cargo is not available on PATH")
    fi
    if [[ "${#missing[@]}" -ne 0 ]]; then
      record_gate_failure "$mode_name" journey_prerequisites "${missing[@]}"
      {
        printf 'lane=signoff-journeys\n'
        printf 'result=FAIL\n'
        printf 'reason=missing_prerequisites\n'
        printf 'stages=prerequisites,journey_signoff_test\n'
        printf 'owns=a_journeys_config_cli_and_worktree_owner_doc\n'
        printf 'note=worktree_pty_remains_env_gated_HARNESS_TUI_PTY_SIGNOFF\n'
      } >"${journey_artifacts_dir}/journey-lane-verdict.txt"
      return 1
    fi
  fi

  run_stage "$mode_name" journey_signoff_owner_present "$repo_root" test -f "$journey_test_path"
  run_stage "$mode_name" journey_signoff_test "$repo_root" \
    env HARNESS_JOURNEY_STRICT=1 HARNESS_JOURNEY_ARTIFACT_DIR="$journey_artifacts_dir" \
    cargo nextest run -p harness --test journey_signoff_test

  local mode_failed=0
  local stage_status
  for stage_status in "$(mode_dir_for signoff-journeys)"/stages/*/status.txt; do
    if [[ -f "$stage_status" ]] && grep -q '^result=FAIL$' "$stage_status"; then
      mode_failed=1
      break
    fi
  done
  if [[ "$mode_failed" -eq 0 ]]; then
    {
      printf 'lane=signoff-journeys\n'
      printf 'result=PASS\n'
      printf 'reason=owners_green\n'
      printf 'stages=prerequisites,journey_signoff_test\n'
      printf 'owns=a_journeys_config_cli_and_worktree_owner_doc\n'
      printf 'note=rows_remain_incomplete_until_full_L1_L6;worktree_pty_env_gated\n'
    } >"${journey_artifacts_dir}/journey-lane-verdict.txt"
  else
    {
      printf 'lane=signoff-journeys\n'
      printf 'result=FAIL\n'
      printf 'reason=stage_failure\n'
      printf 'stages=prerequisites,journey_signoff_test\n'
      printf 'owns=a_journeys_config_cli_and_worktree_owner_doc\n'
      printf 'note=rows_remain_incomplete_until_full_L1_L6;worktree_pty_env_gated\n'
    } >"${journey_artifacts_dir}/journey-lane-verdict.txt"
    return 1
  fi
}

run_signoff_parity() {
  local mode_name="signoff-parity"
  local manifest_rel="docs/reference/tui-reference-parity-manifest.v1.json"
  local manifest_path="${repo_root}/${manifest_rel}"
  local manifest_test_rel="crates/harness-tui/tests/reference_parity_manifest_test.rs"
  local manifest_test_path="${repo_root}/${manifest_test_rel}"
  local cells_test_rel="crates/harness-tui/tests/reference_parity_cells_test.rs"
  local pixels_test_rel="crates/harness-tui/tests/reference_parity_pixels_test.rs"
  local pty_test_rel="crates/harness-tui/tests/reference_parity_pty_test.rs"
  local presentation_receipt_test_rel="crates/harness-testkit/tests/tui_fidelity_presentation_receipt_test.rs"
  local presentation_runner_test_rel="crates/harness-testkit/tests/tui_fidelity_runner_test.rs"
  local reference_binary_rel="inspirations/grok-build/target/debug/xai-grok-pager"
  local reference_binary_path="${repo_root}/${reference_binary_rel}"
  # Pinned reference binary sha256 (must match $.reference.binary_sha256 in the manifest).
  local reference_binary_sha256="883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5"
  local parity_evidence_root
  parity_evidence_root="$(stage_dir_for signoff-parity parity_evidence)"
  local parity_artifacts_dir="${parity_evidence_root}/artifacts"
  local reference_pin_path="${parity_evidence_root}/reference-binary-sha256.txt"
  mkdir -p "$parity_evidence_root"
  printf '%s' "$reference_binary_sha256" >"$reference_pin_path"
  mkdir -p "$parity_artifacts_dir"

  if [[ "$dry_run" -eq 0 ]]; then
    local missing=()
    if [[ ! -f "$manifest_path" ]]; then
      missing+=("${manifest_rel} (independent dual-binary cells/pixels/PTY manifest; tui-signoff-manifest.v1.json does not own this lane)")
    fi
    if ! command -v cargo >/dev/null 2>&1; then
      missing+=("cargo is not available on PATH")
    fi
    for owner_rel in \
      "$manifest_test_rel" \
      "$cells_test_rel" \
      "$pixels_test_rel" \
      "$pty_test_rel" \
      "$presentation_receipt_test_rel" \
      "$presentation_runner_test_rel" \
      "crates/harness-tui/tests/reference_parity_first_slice_test.rs" \
      "crates/harness-tui/tests/reference_parity_perm_question_test.rs" \
      "crates/harness-tui/tests/reference_parity_tx_shell_test.rs" \
      "crates/harness-tui/tests/reference_parity_responsive_test.rs" \
      "crates/harness-tui/tests/p0_parity_contract_test.rs" \
      "crates/harness-tui/tests/shell_topology_contract_test.rs"
    do
      if [[ ! -f "${repo_root}/${owner_rel}" ]]; then
        missing+=("missing owner ${owner_rel} (silent skip is forbidden)")
      fi
    done
    if [[ "${#missing[@]}" -ne 0 ]]; then
      record_gate_failure "$mode_name" parity_prerequisites "${missing[@]}"
      write_signoff_parity_verdict "$parity_artifacts_dir" FAIL missing_prerequisites
      return 1
    fi
  fi

  run_stage "$mode_name" reference_parity_manifest_present "$repo_root" test -f "$manifest_path"

  # Presence + digest of the pinned reference binary only; never rebuild or copy it.
  run_stage "$mode_name" reference_binary_present "$repo_root" \
    bash -c 'test -f "$0" || { echo "missing pinned reference binary $0" >&2; exit 1; }; actual="$(sha256sum "$0" | cut -d" " -f1)"; expected="$(cat "$1")"; if [ "$actual" != "$expected" ]; then echo "reference binary digest mismatch: actual=$actual expected=$expected" >&2; exit 1; fi' \
    "$reference_binary_path" "$reference_pin_path"

  # Generate fresh L3 captures by invoking the capture scripts. Each script
  # runs the Harness binary through a real PTY capture and writes terminal.png,
  # terminal.txt, terminal-ansi.txt, and metadata.json into the evidence root.
  # Requires Chrome/Chromium for PNG capture (set CHROME_BIN or install
  # google-chrome/chromium). If a capture script fails, the lane fails with
  # a clear error — no silent skip.
  # Evidence is generated fresh per run; the parity contract prohibits reuse.
  run_stage "$mode_name" reference_parity_capture_shell_scroll "$repo_root" \
    env EVIDENCE_DIR="$parity_artifacts_dir/actual/harness-shell-live_scroll-pinned-v1" \
    bash scripts/tui-parity/capture-shell-scroll-l3.sh

  run_stage "$mode_name" reference_parity_capture_question_stream "$repo_root" \
    env EVIDENCE_DIR="$parity_artifacts_dir/actual/harness-shell-question-pinned-v1" \
    bash scripts/tui-parity/capture-question-stream-l3.sh

  run_stage "$mode_name" reference_parity_capture_resp_idle "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-resp-idle-shell-l3.sh

  run_stage "$mode_name" reference_parity_capture_perm_empty_draft "$repo_root" \
    env EVIDENCE_DIR="$parity_artifacts_dir/actual/harness-pty-perm-empty-draft-120x32-v1" \
    bash scripts/tui-parity/capture-perm-empty-draft-l3.sh

  run_stage "$mode_name" reference_parity_capture_startup_welcome "$repo_root" \
    env EVIDENCE_DIR="$parity_artifacts_dir/actual/harness-startup-v24" \
    bash scripts/tui-parity/capture-startup-welcome-l3.sh

  run_stage "$mode_name" reference_parity_capture_draft_composer "$repo_root" \
    env EVIDENCE_DIR="$parity_artifacts_dir/actual/harness-draft-v23" \
    bash scripts/tui-parity/capture-draft-composer-l3.sh

  run_stage "$mode_name" reference_parity_capture_shell_lifecycle "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-shell-lifecycle-l3.sh

  # Fresh nonvisual journey L3 captures (one stage per journey row): each
  # stage runs the A-JOURNEYS owner tests in crates/harness/tests/
  # journey_signoff_test.rs in self-contained mode (CLI/backend evidence
  # only — no Chrome, no pixel PNG), then relocates the generated
  # journey-*-v1 evidence directory into the lane's fresh evidence root and
  # writes a provenance metadata.json (behavior_id + generating_command) for
  # the strict provenance validator. A failed capture fails the lane — no
  # silent skip.
  run_stage "$mode_name" reference_parity_capture_journey_worktree_owner "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh worktree-owner

  run_stage "$mode_name" reference_parity_capture_journey_config_show_effective "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh config-show-effective

  run_stage "$mode_name" reference_parity_capture_journey_config_sources_explain "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh config-sources-explain

  run_stage "$mode_name" reference_parity_capture_journey_wait_any_all "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh wait-any-all

  run_stage "$mode_name" reference_parity_capture_journey_memory_cli "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh memory-cli

  run_stage "$mode_name" reference_parity_capture_journey_folder_trust_deny "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh folder-trust-deny

  run_stage "$mode_name" reference_parity_capture_journey_always_approve_mode "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh always-approve-mode

  run_stage "$mode_name" reference_parity_capture_journey_settings_editor "$repo_root" \
    env EVIDENCE_BASE="$parity_artifacts_dir/actual" \
    bash scripts/tui-parity/capture-journey-l3.sh settings-editor

  # Generate the reference-freeze receipt fresh into the evidence root. The
  # receipt establishes pinned-reference-binary provenance for this run
  # (Contract §5.1: each signoff run creates a new evidence root that records
  # the reference path, SHA-256, version, and reference revision). The strict
  # provenance validator (reference_parity_provenance::verify_freeze_receipt)
  # fails closed when this receipt is missing or its pinned digests drift.
  run_stage "$mode_name" reference_parity_freeze_receipt "$repo_root" \
    bash -c 'mkdir -p "$0/receipts" && python3 -c "
import json, os, sys
out = os.path.join(sys.argv[1], \"receipts\", \"reference-freeze.receipt.json\")
receipt = {
    \"schema_version\": \"harness-tui-reference-freeze-receipt-v1\",
    \"receipt_id\": \"reference-freeze\",
    \"scenario\": \"startup_welcome_120x32\",
    \"viewport\": {\"cols\": 120, \"rows\": 32},
    \"global_pinned_reference\": {
        \"binary_sha256\": \"883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5\",
        \"reference_revision\": \"c1b5909ec707c069f1d21a93917af044e71da0d7\",
        \"version\": \"grok 0.1.220-alpha.4 (c1b5909) [stable]\"
    },
    \"freeze_txt_sha256\": \"1a5f24dc9be953df160e8d2bcb661f6f2d8dc7845021c3153cd415ab3889ca58\",
    \"freeze_png_sha256\": \"0830427651ae47645ea3ea49b532ef7ea29a69c3140f140d7df201f5093d6016\"
}

os.makedirs(os.path.dirname(out), exist_ok=True)
with open(out, \"w\") as f:
    json.dump(receipt, f, indent=2)
    f.write(\"\n\")
print(f\"wrote freeze receipt {out}\")
" "$0"' \
    "$parity_artifacts_dir"

  # Fresh terminal capability L3 capture (shared by the four TERM-CAP-* rows):
  # runs the env-gated terminal_capability_matrix_capture_test, which derives
  # the Harness negotiated terminal mode set, asserts exact parity with the
  # pinned reference binary (fail-closed), and writes the capability matrix
  # receipt plus a provenance metadata.json. Journey-style L3+receipt contract
  # — no Chrome/PNG. A failed capture fails the lane (no silent skip).
  run_stage "$mode_name" reference_parity_capture_term_cap "$repo_root" \
    env EVIDENCE_DIR="$parity_artifacts_dir/actual/harness-term-cap-v1" \
    bash scripts/tui-parity/capture-term-cap-l3.sh

  # Generate canonical L1/L4/L5/L6 evidence layers from the pinned reference
  # lab and the fresh L3 captures produced above. Fail-closed: any missing
  # reference freeze, capture, or receipt causes the lane to fail.
  local claimed_visual_rows
  claimed_visual_rows="$(python3 -c 'import json,sys; manifest=json.load(open(sys.argv[1])); print(",".join(row["behavior_id"] for row in manifest["rows"] if row.get("status") in {"pass", "diverged"} and row.get("row_kind", "visual") == "visual"))' "$manifest_path")"
  run_stage "$mode_name" reference_parity_generate_evidence_layers "$repo_root" \
    env HARNESS_TUI_PARITY_VISUAL_ROWS="$claimed_visual_rows" \
    python3 scripts/tui-parity/generate-evidence-layers.py \
      --lab "artifacts/qa-evidence/20260717-tui-reference-parity" \
      --out "$parity_artifacts_dir" \
      --lane

  run_stage "$mode_name" reference_parity_manifest_test "$repo_root" \
    env HARNESS_TUI_PARITY_ARTIFACT_DIR="$parity_artifacts_dir" \
    cargo nextest run -p harness-tui --test reference_parity_manifest_test

  run_stage "$mode_name" p0_parity_contract_test "$repo_root" \
    cargo nextest run -p harness-tui --test p0_parity_contract_test
  run_stage "$mode_name" shell_topology_contract_test "$repo_root" \
    cargo nextest run -p harness-tui --test shell_topology_contract_test

  run_stage "$mode_name" reference_parity_cells_test "$repo_root" \
    env HARNESS_TUI_PARITY_ARTIFACT_DIR="$parity_artifacts_dir" HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_cells_test
  run_stage "$mode_name" reference_parity_pixels_test "$repo_root" \
    env HARNESS_TUI_PARITY_ARTIFACT_DIR="$parity_artifacts_dir" HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_pixels_test
  run_stage "$mode_name" reference_parity_first_slice_test "$repo_root" \
    env HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_first_slice_test
  run_stage "$mode_name" reference_parity_perm_question_test "$repo_root" \
    env HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_perm_question_test
  run_stage "$mode_name" reference_parity_tx_shell_test "$repo_root" \
    env HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_tx_shell_test
  run_stage "$mode_name" reference_parity_responsive_test "$repo_root" \
    env HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_responsive_test
  run_stage "$mode_name" reference_parity_pty_test "$repo_root" \
    env RUST_TEST_THREADS=1 HARNESS_TUI_PTY_SIGNOFF=1 HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_pty_test --test-threads 1

  local presentation_artifacts_dir
  presentation_artifacts_dir="$(stage_dir_for signoff-parity presentation_telemetry)/artifacts"
  mkdir -p "$presentation_artifacts_dir"
  run_stage "$mode_name" presentation_telemetry "$repo_root" \
    bash -c 'cargo nextest run -p harness-testkit --test tui_fidelity_presentation_receipt_test && RUST_TEST_THREADS=1 HARNESS_PACKET1_EVIDENCE_DIR="$0/complete" cargo nextest run -p harness-testkit --test tui_fidelity_runner_test --test-threads 1 packet1_complete_receipt_passes_all_gates && RUST_TEST_THREADS=1 HARNESS_PACKET1_EVIDENCE_DIR="$0/defects" cargo nextest run -p harness-testkit --test tui_fidelity_runner_test --test-threads 1 packet1_controlled_defect_matrix' \
    "$presentation_artifacts_dir"

  # Final fail-closed provenance gate: the strict validator must accept the
  # fresh evidence root (L1-L6 files present, capture/freeze/receipt digests
  # hash-matched, capture metadata matched to the owning rows).
  run_stage "$mode_name" reference_parity_manifest_evidence "$repo_root" \
    env HARNESS_TUI_PARITY_ARTIFACT_DIR="$parity_artifacts_dir" HARNESS_TUI_PARITY_STRICT=1 \
    cargo nextest run -p harness-tui --test reference_parity_evidence_test

  local mode_failed=0
  local stage_status
  for stage_status in "$(mode_dir_for signoff-parity)"/stages/*/status.txt; do
    if [[ -f "$stage_status" ]] && grep -q '^result=FAIL$' "$stage_status"; then
      mode_failed=1
      break
    fi
  done

  if [[ "$dry_run" -eq 1 ]]; then
    write_signoff_parity_verdict "$parity_artifacts_dir" DRY-RUN command_not_executed
  elif [[ "$mode_failed" -ne 0 ]]; then
    write_signoff_parity_verdict "$parity_artifacts_dir" FAIL stage_failure
    fail_count=$((fail_count + 1))
  else
    write_signoff_parity_verdict "$parity_artifacts_dir" PASS all_required_stages_passed
    # write_signoff_parity_verdict may downgrade PASS→FAIL when the fresh root has
    # zero artifacts; re-read the verdict to reflect that in the exit count.
    local final_verdict
    final_verdict="$(grep '^verdict=' "${parity_artifacts_dir}/parity-lane-verdict.txt" | head -1 | cut -d= -f2)"
    if [[ "$final_verdict" == "FAIL" ]]; then
      fail_count=$((fail_count + 1))
    fi
  fi
}

run_signoff_packet2() {
  local mode_name="signoff-packet2"
  local mode_root
  mode_root="$(mode_dir_for "$mode_name")"
  mkdir -p "$mode_root"
  local commands_path="$mode_root/commands.txt"
  local target_dir="$repo_root/target/packet2-candidate"
  local candidate_receipt="$mode_root/candidate-receipt.json"
  local runner="$target_dir/debug/tui-fidelity"
  local candidate="$target_dir/debug/harness"
  local aggregate="$target_dir/debug/tui_fidelity_aggregate"

  if [[ -z "$reference_bin" || -z "$reference_receipt" || -z "$reference_root" ]]; then
    printf 'signoff-packet2 preflight: --reference-bin, --reference-receipt, and --reference-root are required\n' >&2
    fail_count=$((fail_count + 1))
    return 2
  fi
  if [[ "$reference_bin" != /* || "$reference_receipt" != /* || "$reference_root" != /* ]]; then
    printf 'signoff-packet2 preflight: reference inputs must be absolute paths\n' >&2
    fail_count=$((fail_count + 1))
    return 2
  fi
  if [[ "$dry_run" -eq 0 ]]; then
    if [[ ! -x "$reference_bin" || ! -f "$reference_receipt" || ! -d "$reference_root" ]]; then
      printf 'signoff-packet2 preflight: pinned reference binary, receipt, or root is missing\n' >&2
      fail_count=$((fail_count + 1))
      return 2
    fi
    if [[ "$(git -C "$reference_root" rev-parse HEAD 2>/dev/null)" != "be713136d2a69080743a3f6b3c72077057e5948f" ]]; then
      printf 'signoff-packet2 preflight: reference revision mismatch\n' >&2
      fail_count=$((fail_count + 1))
      return 2
    fi
  fi

  : >"$commands_path"
  local run_root
  for ordinal in 1 2 3 4 5; do
    run_root="$mode_root/run-$ordinal"
    write_quoted_command_line "$commands_path" \
      "$runner" compare --scenario packet2-sustained-stream \
      --acceptance packet2-scheduling --reference-bin "$reference_bin" \
      --reference-receipt "$reference_receipt" --reference-root "$reference_root" \
      --harness-bin "$candidate" --candidate-receipt "$candidate_receipt" \
      --evidence-dir "$run_root"
  done
  write_quoted_command_line "$commands_path" "$aggregate" --profile packet2-scheduling \
    "$mode_root/run-1" "$mode_root/run-2" "$mode_root/run-3" "$mode_root/run-4" "$mode_root/run-5"

  run_stage "$mode_name" build_candidate "$repo_root" \
    bash scripts/tui-fidelity/build-candidate.sh \
      --target-dir target/packet2-candidate --receipt "$candidate_receipt"
  for ordinal in 1 2 3 4 5; do
    run_stage "$mode_name" "packet2_compare_$ordinal" "$repo_root" \
      "$runner" compare --scenario packet2-sustained-stream \
      --acceptance packet2-scheduling --reference-bin "$reference_bin" \
      --reference-receipt "$reference_receipt" --reference-root "$reference_root" \
      --harness-bin "$candidate" --candidate-receipt "$candidate_receipt" \
      --evidence-dir "$mode_root/run-$ordinal"
  done
  run_stage "$mode_name" packet2_aggregate "$repo_root" \
    "$aggregate" --profile packet2-scheduling \
      "$mode_root/run-1" "$mode_root/run-2" "$mode_root/run-3" "$mode_root/run-4" "$mode_root/run-5"
  run_stage "$mode_name" packet2_source_guard "$repo_root" \
    git diff --exit-code 26ef1839 -- crates/harness-tui/tests/snapshots \
      crates/harness-tui/src/ui.rs crates/harness-tui/src/layout.rs crates/harness-tui/src/theme.rs

  local verdict="PASS"
  if find "$mode_root/stages" -name status.txt -exec grep -l '^result=FAIL$' {} + | grep -q .; then
    verdict="FAIL"
    fail_count=$((fail_count + 1))
  elif [[ "$dry_run" -eq 1 ]]; then
    verdict="DRY-RUN"
  fi
  printf 'schema=harness-signoff-packet2-verdict-v1\nverdict=%s\nruns=5\nprofile=packet2-scheduling\n' \
    "$verdict" >"$mode_root/lane-verdict.txt"
}

write_signoff_parity_verdict() {
  local artifacts_dir="$1"
  local verdict="$2"
  local note="$3"
  local verdict_path="${artifacts_dir}/parity-lane-verdict.txt"
  mkdir -p "$artifacts_dir"

  local git_rev
  git_rev="$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || echo 'unknown')"
  local manifest_path="${repo_root}/docs/reference/tui-reference-parity-manifest.v1.json"
  local manifest_digest
  manifest_digest="$(sha256sum "$manifest_path" 2>/dev/null | cut -d' ' -f1 || echo 'missing')"
  local parity_complete="false"
  if [[ -f "$manifest_path" ]]; then
    parity_complete="$(python3 -c 'import json,sys; rows=json.load(open(sys.argv[1]))["rows"]; print("true" if all(row.get("status") in {"pass", "diverged", "excluded"} for row in rows) else "false")' "$manifest_path" 2>/dev/null || echo 'false')"
  fi

  # Fail-closed: PASS requires at least one non-verdict artifact in the evidence dir
  if [[ "$verdict" == "PASS" ]]; then
    local artifact_count
    artifact_count="$(find "$artifacts_dir" -type f ! -name 'parity-lane-verdict.txt' | wc -l | tr -d ' ')"
    if [[ "$artifact_count" -eq 0 ]]; then
      verdict="FAIL"
      note="no_evidence_artifacts_in_fresh_root"
    fi
  fi

  cat >"$verdict_path" <<EOF
schema=harness-signoff-parity-verdict-v1
mode=signoff-parity
verdict=${verdict}
parity_complete=${parity_complete}
note=${note}
git_revision=${git_rev}
manifest_sha256=${manifest_digest}
owns=dual_binary_cells_and_pixels
stages=manifest,reference_binary,p0_contract,shell_topology,cells,pixels,first_slice,perm_question,tx_shell,responsive,pty_with_signoff,presentation_telemetry,evidence_provenance
does_not_own=tui-signoff-manifest.v1.json
manifest=docs/reference/tui-reference-parity-manifest.v1.json
EOF
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
    signoff-parity)
      run_signoff_parity
      ;;
    signoff-packet2)
      run_signoff_packet2
      ;;
    signoff-journeys)
      run_signoff_journeys
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
