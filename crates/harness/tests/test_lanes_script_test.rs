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
fn release_blocker_taxonomy_maps_categories_to_real_lanes() {
    // arrange
    let blockers =
        fs::read_to_string(repo_root().join("docs/release-blockers.md")).unwrap_or_abort();
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let modes = lane_modes_from_script(&script);
    let rows = markdown_table_rows(&blockers);

    // assert
    for category in [
        "correctness",
        "safety",
        "UX",
        "docs",
        "provider",
        "performance",
        "evidence",
    ] {
        let row = rows
            .iter()
            .find(|row| row.first().is_some_and(|cell| cell == category))
            .unwrap_or_else(|| panic!("release blocker taxonomy missing `{category}`"));
        let lanes = backticked_values(row.get(2).unwrap_or_abort())
            .into_iter()
            .filter(|lane| modes.contains(lane.as_str()))
            .collect::<Vec<_>>();
        assert!(
            !lanes.is_empty(),
            "release blocker category `{category}` must map to at least one scripts/test-lanes.sh mode"
        );
        for lane in lanes {
            assert!(
                modes.contains(lane.as_str()),
                "release blocker lane `{lane}` is not declared by scripts/test-lanes.sh"
            );
        }
    }

    assert!(blockers.contains("## Local development aids"));
    assert!(
        blockers.contains("A green doctor report is runtime health, not full roadmap completion")
    );
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

fn markdown_table_rows(doc: &str) -> Vec<Vec<String>> {
    doc.lines()
        .filter(|line| line.trim_start().starts_with('|'))
        .filter(|line| !line.contains("|---"))
        .map(|line| {
            line.trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect::<Vec<_>>()
        })
        .filter(|row| row.len() >= 3 && row.first().is_none_or(|cell| cell != "Category"))
        .collect()
}

fn backticked_values(cell: &str) -> Vec<String> {
    cell.split('`')
        .enumerate()
        .filter_map(|(index, value)| (index % 2 == 1).then_some(value.to_string()))
        .collect()
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
