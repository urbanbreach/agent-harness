use harness::UnwrapOrAbort;
use std::collections::BTreeSet;
use std::fs;

mod common;

use common::repo_root;

#[test]
fn test_lanes_exports_artifact_dir_for_performance_stage() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh"))
        // act
        .unwrap_or_abort();

    // assert
    assert!(script.contains("perf_artifacts_dir=\"$(stage_dir_for perf nextest_perf)/artifacts\""));
    assert!(script.contains("mkdir -p \"$perf_artifacts_dir\""));
    assert!(script.contains("HARNESS_PERF_ARTIFACT_DIR=\"$perf_artifacts_dir\""));
}

#[test]
fn test_lanes_ci_profile_is_defined_for_fast_and_integration() {
    // arrange
    let root = repo_root();
    let script = fs::read_to_string(root.join("scripts/test-lanes.sh")).unwrap_or_abort();
    let nextest_config = fs::read_to_string(root.join(".config/nextest.toml")).unwrap_or_abort();

    // act
    let ci_profile_is_wired = script
        .contains("cargo nextest run --profile ci --workspace --all-features")
        && script.contains(
            "cargo nextest run --profile ci --workspace --all-features --partition hash:1/2",
        )
        && script.contains(
            "cargo nextest run --profile ci --workspace --all-features --partition hash:2/2",
        );

    // assert
    assert!(ci_profile_is_wired);
    assert!(nextest_config.contains("[profile.default]"));
    assert!(nextest_config.contains("[profile.ci]\ninherits = \"default\""));
    assert!(nextest_config.contains("junit = { path = \"target/nextest/ci/junit.xml\" }"));
}

#[test]
fn test_lanes_declares_core_evidence_modes() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let modes = lane_modes_from_script(&script);

    // assert
    for mode in [
        "fast",
        "quality-gates",
        "integration",
        "simulation",
        "perf",
        "signoff-binary",
        "signoff-pty",
        "signoff-live",
        "signoff-native",
    ] {
        assert!(
            modes.contains(mode),
            "scripts/test-lanes.sh must declare mode `{mode}`"
        );
    }
}

#[test]
fn test_lanes_runs_perf_artifact_freshness_gate() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();
    let checker =
        fs::read_to_string(repo_root().join("scripts/check-perf-artifacts.py")).unwrap_or_abort();

    // act
    let stage_guard_present = script.contains("perf_artifact_freshness")
        && script.contains("scripts/check-perf-artifacts.py")
        && script.contains("--artifact-dir");
    let freshness_checker_present = checker.contains("large-session-surfaces.json")
        && checker.contains("harness-large-session-perf-v1")
        && checker.contains("stale");

    // assert
    assert!(stage_guard_present);
    assert!(freshness_checker_present);
}

#[test]
fn signoff_binary_exports_smoke_artifact_dir() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let binary_smoke_artifact_dir = script.contains(
        "HARNESS_BINARY_SMOKE_ARTIFACT_DIR=\"$binary_smoke_artifacts_dir\"",
    ) && script.contains("binary_smoke_artifacts_dir=\"$(stage_dir_for signoff-binary harness_binary_smoke)/artifacts\"");

    // assert
    assert!(binary_smoke_artifact_dir);
}

#[test]
fn signoff_pty_records_happy_path_artifact_dir() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let happy_path_stage = script.contains(
        "tui_happy_path_artifacts_dir=\"$(stage_dir_for signoff-pty harness_tui_happy_path_pty)/artifacts\"",
    ) && script.contains("HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR=\"$tui_happy_path_artifacts_dir\"")
        && script.contains("cargo nextest run -p harness --test pty_happy_path_recorded")
        && script.contains("harness_tui_happy_path_pty");
    // assert
    assert!(happy_path_stage);
}

#[test]
fn signoff_pty_mode_is_fail_closed() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let body = function_body(&script, "run_signoff_pty");
    let help_declares = script.contains("signoff-pty")
        && script.contains("Strict fail-closed deterministic PTY signoff");
    let owns_owners = body.contains("crates/harness-testkit/tests/pty_e2e.rs")
        && body.contains("crates/harness-tui/tests/pty_e2e.rs")
        && body.contains("crates/harness/tests/pty_happy_path_recorded.rs")
        && body.contains("HARNESS_TUI_PTY_SIGNOFF=1");
    let fail_closed = !body.contains("|| true")
        && body.contains("silent skip is forbidden")
        && body.contains("pty-lane-verdict.txt");
    let lists_stages = body.contains("stages=testkit_pty,tui_pty,happy_path");

    // assert
    assert!(
        help_declares,
        "help/usage must document fail-closed signoff-pty"
    );
    assert!(
        owns_owners,
        "signoff-pty must gate PTY owners and the recorded happy path"
    );
    assert!(
        fail_closed,
        "signoff-pty must be fail-closed (no || true; missing owner fails)"
    );
    assert!(
        lists_stages,
        "signoff-pty verdict must list the stages it actually executed"
    );
}

#[test]
fn engine_metrics_script_declares_the_versioned_baseline_contract() {
    // arrange
    let script =
        fs::read_to_string(repo_root().join("scripts/engine-metrics.sh")).unwrap_or_abort();

    // act
    let required_contract_tokens = [
        "engine-metrics-v1",
        "--output",
        "--baseline",
        "production_loc",
        "frozen_overlap",
        "event_variants",
        "compaction_variants",
        "reducer_count",
        "size_ok",
        "representative_log",
        "list_inspect_latency",
        "long_session_context_build",
        "model_resolution",
        "provider_context/restore.rs",
        "CompactionFailed",
        "unavailable",
    ];

    // assert
    for token in required_contract_tokens {
        assert!(
            script.contains(token),
            "engine metrics script missing contract token `{token}`"
        );
    }
}

fn lane_modes_from_script(script: &str) -> BTreeSet<String> {
    let mut modes = BTreeSet::new();
    for line in script.lines() {
        let trimmed = line.trim();
        if !trimmed.ends_with(')') || trimmed.starts_with('$') || trimmed.starts_with("*") {
            continue;
        }
        let arm = trimmed.trim_end_matches(')');
        for mode in arm.split('|') {
            if mode
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
            {
                modes.insert(mode.to_string());
            }
        }
    }
    modes
}

fn function_body<'a>(script: &'a str, name: &str) -> &'a str {
    let marker = format!("{name}() {{");
    assert!(
        script.contains(&marker),
        "scripts/test-lanes.sh must define {name}()"
    );
    let start = script.find(&marker).unwrap_or_abort();
    let after = &script[start + marker.len()..];
    let mut depth = 1usize;
    let mut end = 0usize;
    for (idx, ch) in after.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end = idx;
                    break;
                }
            }
            _ => {}
        }
    }
    assert!(
        end > 0,
        "scripts/test-lanes.sh function {name}() must have a matching closing brace"
    );
    &after[..end]
}
