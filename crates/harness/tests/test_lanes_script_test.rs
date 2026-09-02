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
fn signoff_pty_wires_p1_04_native_and_xterm_owners_with_artifact_roots() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let p1_04_contract = script.contains(
        "p1_04_artifacts_dir=\"$(stage_dir_for signoff-pty harness_tui_p1_04_pty_recorded)/artifacts\"",
    ) && script.contains("HARNESS_P1_04_ARTIFACT_DIR=\"$p1_04_artifacts_dir\"")
        && script.contains("cargo nextest run -p harness-tui --test p1_04_pty_recorded")
        && script.contains("p1_04_xterm_tests")
        && script.contains("--scenario p1-04-responsive-feedback");

    // assert
    assert!(p1_04_contract);
    assert!(script.contains("run_stage \"$mode_name\" p1_04_xterm_tests"));
    assert!(!script.contains("p1_04_pty_recorded --test-threads 1 --ignore-default-filter || true"));
}

#[test]
fn signoff_pty_wires_p1_03_native_and_xterm_owners_with_artifact_roots() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let p1_03_contract = script.contains(
        "p1_03_artifacts_dir=\"$(stage_dir_for signoff-pty harness_tui_p1_03_pty_recorded)/artifacts\"",
    ) && script.contains("HARNESS_P1_03_ARTIFACT_DIR=\"$p1_03_artifacts_dir\"")
        && script.contains("cargo nextest run -p harness-tui --test p1_03_pty_recorded")
        && script.contains("p1_03_xterm_tests")
        && script.contains("--scenario p1-03-startup-reveal");

    // assert
    assert!(p1_03_contract);
    assert!(script.contains("run_stage \"$mode_name\" p1_03_xterm_tests"));
    assert!(!script.contains("p1_03_pty_recorded --test-threads 1 --ignore-default-filter || true"));
}

#[test]
fn signoff_pty_dry_run_emits_stage_artifact_and_fail_closed_contract() {
    // arrange
    let root = repo_root();
    let artifact_root = tempfile::tempdir().unwrap_or_abort();
    let script = root.join("scripts/test-lanes.sh");

    // act
    let output = std::process::Command::new("bash")
        .arg(&script)
        .arg("signoff-pty")
        .arg("--dry-run")
        .arg("--artifact-dir")
        .arg(artifact_root.path())
        .current_dir(&root)
        .output()
        .unwrap_or_abort();

    // assert
    assert!(output.status.success(), "lane failed: {output:?}");
    let stages = [
        "harness_testkit_pty_e2e",
        "harness_tui_pty_e2e",
        "harness_tui_p0_03_pty_recorded",
        "harness_tui_p0_04_pty_recorded",
        "harness_tui_p1_02_pty_recorded",
        "harness_tui_p1_03_pty_recorded",
        "harness_tui_p1_04_pty_recorded",
        "harness_tui_happy_path_pty",
        "p0_06_xterm_dependencies",
        "p0_06_xterm_tests",
        "p1_02_xterm_tests",
        "p1_03_xterm_tests",
        "p1_04_xterm_tests",
        "xterm_harness_binary",
        "p0_06_xterm_80x24",
        "p0_06_xterm_120x40",
        "p0_06_xterm_160x50",
        "p1_02_xterm_80x24",
        "p1_02_xterm_120x40",
        "p1_02_xterm_160x50",
        "p1_03_xterm_80x24",
        "p1_03_xterm_120x40",
        "p1_03_xterm_160x50",
        "p1_03_xterm_basic_ascii",
        "p1_04_xterm_80x24",
        "p1_04_xterm_120x40",
        "p1_04_xterm_160x50",
    ];
    let summary = fs::read_to_string(artifact_root.path().join("summary.txt")).unwrap_or_abort();
    for stage in stages {
        assert!(summary.contains(&format!("signoff-pty {stage} DRY-RUN")));
        assert!(
            artifact_root
                .path()
                .join(format!("signoff-pty/stages/{stage}/status.txt"))
                .is_file(),
            "missing status artifact for {stage}"
        );
    }
    let p0_06_artifacts = artifact_root
        .path()
        .join("signoff-pty/stages/harness_tui_pty_e2e/artifacts/p0-06");
    assert!(p0_06_artifacts.is_dir());
    let p1_03_artifacts = artifact_root
        .path()
        .join("signoff-pty/stages/harness_tui_p1_03_pty_recorded/artifacts");
    assert!(p1_03_artifacts.is_dir());
    let p1_04_artifacts = artifact_root
        .path()
        .join("signoff-pty/stages/harness_tui_p1_04_pty_recorded/artifacts");
    assert!(p1_04_artifacts.is_dir());
    for stage in [
        "p1_03_xterm_80x24",
        "p1_03_xterm_120x40",
        "p1_03_xterm_160x50",
        "p1_03_xterm_basic_ascii",
        "p1_04_xterm_80x24",
        "p1_04_xterm_120x40",
        "p1_04_xterm_160x50",
    ] {
        assert!(
            artifact_root
                .path()
                .join(format!("signoff-pty/stages/{stage}/artifacts"))
                .is_dir(),
            "missing artifact root for {stage}"
        );
    }
    let verdict = fs::read_to_string(
        artifact_root
            .path()
            .join("signoff-pty/stages/harness_tui_happy_path_pty/artifacts/pty-lane-verdict.txt"),
    )
    .unwrap_or_abort();
    assert!(verdict.contains("result=PASS"));
    assert!(verdict.contains("reason=owners_green"));
    assert!(verdict.contains("p1_04_xterm_80x24"));
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
