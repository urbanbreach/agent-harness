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
fn test_lanes_profile_allows_bounded_completion_for_reference_guard_owners() {
    // arrange
    let nextest_config =
        fs::read_to_string(repo_root().join(".config/nextest.toml")).unwrap_or_abort();

    // act
    let affected_owner_selectors = [
        "reference_receipt_mutations_fail_closed",
        "source_guard_accepts_(clean_pinned_reference|current_code_input|current_manifest_input|fresh_runtime_output|relative_canonical_reference)",
        "source_guard_rejects_stale_receipt",
        "cached_reference_(requires_exact_presentation_identity|rejects_trace_or_schedule_drift|rejects_trace_artifact_drift)",
        "compare_(writes_dual_runtime_checkpoint_and_cleanup_receipts|rejects_timeout_premature_exit_and_forced_kill_completion)",
        "lifecycle_cases::(hanging_renderer_times_out_and_is_reaped_repeatedly|preexisting_runtime_root_and_sentinel_are_preserved)",
        "packet1_(complete_receipt_passes_all_gates|provenance_rejects_every_bound_identity_mismatch)",
        "presentation_trace_is_native_for_harness_and_external_for_grok",
        "relative_evidence_path_preserves_harness_native_sidecar_across_runtime_cleanup",
        "runner_packet::(compare_writes_explicit_capture_and_comparison_gate_receipt|packet1_controlled_defect_matrix)",
    ];

    // assert
    assert!(
        nextest_config
            .matches("slow-timeout = { period = \"30s\", terminate-after = 4 }")
            .count()
            >= 4,
        "the reference, source-guard, and runner overrides must permit a bounded 120 seconds"
    );
    for selector in affected_owner_selectors {
        assert!(
            nextest_config.contains(selector),
            "the slow-timeout override must cover {selector}"
        );
    }
}

#[test]
fn test_lanes_profile_serializes_tui_fidelity_runner_process_owners() {
    // arrange
    let nextest_config =
        fs::read_to_string(repo_root().join(".config/nextest.toml")).unwrap_or_abort();

    // act
    let runner_process_owners = "[[profile.default.overrides]]\nfilter = 'binary(=tui_fidelity_runner_test)'\ntest-group = \"process-global-state\"";

    // assert
    assert!(
        nextest_config.contains(runner_process_owners),
        "the PTY/process runner owners must share the serialized process-global-state group"
    );
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
        "signoff-parity",
        "signoff-journeys",
    ] {
        assert!(
            modes.contains(mode),
            "scripts/test-lanes.sh must declare mode `{mode}`"
        );
    }
}

#[test]
fn signoff_parity_mode_is_fail_closed() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let body = function_body(&script, "run_signoff_parity");
    let verdict_writer = function_body(&script, "write_signoff_parity_verdict");
    let help_declares = script.contains("signoff-parity")
        && script.contains("Strict fail-closed dual-binary TUI reference parity");
    let owns_independent_manifest = body
        .contains("docs/reference/tui-reference-parity-manifest.v1.json")
        && body.contains("reference_parity_manifest_present")
        && body.contains("write_signoff_parity_verdict")
        && verdict_writer.contains("parity-lane-verdict.txt")
        && verdict_writer.contains("owns=dual_binary_cells_and_pixels");
    let runs_owners = body.contains("reference_parity_manifest_test")
        && body.contains("p0_parity_contract_test")
        && body.contains("shell_topology_contract_test")
        && body.contains("reference_parity_cells_test")
        && body.contains("reference_parity_pixels_test")
        && body.contains("reference_parity_first_slice_test")
        && body.contains("reference_parity_perm_question_test")
        && body.contains("reference_parity_tx_shell_test")
        && body.contains("reference_parity_responsive_test")
        && body.contains("reference_parity_pty_test")
        && body.contains("reference_binary_present")
        && body.contains("reference-binary-sha256.txt")
        && body.contains("capture-journey-l3.sh")
        && body.contains("reference_parity_manifest_evidence")
        && body.contains("reference_parity_evidence_test")
        && body.contains("presentation_telemetry")
        && body.contains("tui_fidelity_presentation_receipt_test")
        && body.contains("tui_fidelity_runner_test")
        && body.contains("HARNESS_TUI_PTY_SIGNOFF=1");
    let strict_evidence = body.matches("HARNESS_TUI_PARITY_STRICT=1").count() >= 4
        && body.contains("reference_parity_cells_test")
        && body.contains("reference_parity_pixels_test");
    let fail_closed = !body.contains("|| true") && !verdict_writer.contains("|| true");
    let lists_stages = verdict_writer.contains("stages=manifest,reference_binary,p0_contract,shell_topology,cells,pixels,first_slice,perm_question,tx_shell,responsive,pty_with_signoff,presentation_telemetry,evidence_provenance");

    // assert
    assert!(help_declares, "help/usage must document signoff-parity");
    assert!(
        owns_independent_manifest,
        "signoff-parity must gate the independent reference-parity manifest and write a verdict"
    );
    assert!(
        runs_owners,
        "signoff-parity must run cells/pixels/first-slice/tx/responsive/PTY owner stages, not structural-only"
    );
    assert!(
        strict_evidence,
        "signoff-parity must set HARNESS_TUI_PARITY_STRICT=1 for cells/pixels/PTY evidence stages (no soft-skip missing freezes)"
    );
    assert!(
        lists_stages,
        "signoff-parity verdict must list the stages it actually executed"
    );
    assert!(
        fail_closed,
        "signoff-parity must be fail-closed (no || true on its stages)"
    );
    assert!(
        body.contains("tui-signoff-manifest.v1.json does not own this lane")
            || script.contains("tui-signoff-manifest.v1.json does not"),
        "signoff-parity must document that tui-signoff-manifest does not own dual-binary acceptance"
    );
}

#[test]
fn signoff_parity_completion_uses_only_active_sealed_verify_all_receipt() {
    // arrange: the canonical lane script and its final verdict writer.
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act: the completion-authority seam is inspected.
    let verdict_writer = function_body(&script, "write_signoff_parity_verdict");

    // assert: historical rows cannot derive completion, and the receipt bindings are explicit.
    assert!(!verdict_writer.contains("tui-reference-parity-manifest.v1.json"));
    assert!(!verdict_writer.contains("row.get(\"status\")"));
    for binding in [
        "authority_sha256",
        "inventory_sha256",
        "coverage_sha256",
        "verification_receipt_path",
        "verification_receipt_sha256",
    ] {
        assert!(
            verdict_writer.contains(binding),
            "final verdict omits active completion binding {binding}"
        );
    }
    assert!(verdict_writer.contains("profile") && verdict_writer.contains("all"));
    assert!(verdict_writer.contains("sealed"));
    assert!(verdict_writer.contains("skipped"));
}

#[test]
fn signoff_packet2_is_pinned_sequential_exactly_five_and_fail_closed() {
    // arrange: the canonical lane script.
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act: the Packet 2 function body is inspected independently of execution.
    let body = function_body(&script, "run_signoff_packet2");

    // assert: it requires pinned inputs, runs five sequential captures, and aggregates once.
    assert!(script.contains("signoff-packet2"));
    assert!(body.contains("eb267feff13129e568df38fb6fdf0ceb65f735d6"));
    assert!(body.contains("for ordinal in 1 2 3 4 5"));
    assert_eq!(body.matches("for ordinal in 1 2 3 4 5").count(), 2);
    assert!(body.contains("packet2-sustained-stream"));
    assert!(body.contains("--acceptance packet2-scheduling"));
    assert!(body.contains("--profile packet2-scheduling"));
    assert!(body.contains("--reference-receipt"));
    assert!(body.contains("--reference-root"));
    assert!(!body.contains("&\n"));
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
fn signoff_journeys_mode_is_fail_closed() {
    // arrange
    let script = fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).unwrap_or_abort();

    // act
    let body = function_body(&script, "run_signoff_journeys");
    let help_declares = script.contains("signoff-journeys")
        && script.contains("Strict fail-closed A-JOURNEYS scaffolding");
    let owns_owner = body.contains("crates/harness/tests/journey_signoff_test.rs")
        && body.contains("journey_signoff_test")
        && body.contains("HARNESS_JOURNEY_ARTIFACT_DIR")
        && body.contains("HARNESS_JOURNEY_STRICT=1");
    let fail_closed = !body.contains("|| true")
        && body.contains("silent skip is forbidden")
        && body.contains("journey-lane-verdict.txt");

    // assert
    assert!(help_declares, "help/usage must document signoff-journeys");
    assert!(
        owns_owner,
        "signoff-journeys must gate journey_signoff_test and export HARNESS_JOURNEY_ARTIFACT_DIR"
    );
    assert!(
        fail_closed,
        "signoff-journeys must be fail-closed (no || true; missing owner fails)"
    );
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
    let dual_binary_stage = script.contains(
        "dual_binary_artifacts_dir=\"$(stage_dir_for signoff-pty harness_tui_dual_binary_cli_pty)/artifacts\"",
    ) && script.contains("HARNESS_TUI_HAPPY_PATH_ARTIFACT_DIR=\"$dual_binary_artifacts_dir\"")
        && script.contains("HARNESS_TUI_PTY_SIGNOFF=1")
        && script.contains("dual_binary_cli_pty")
        && script.contains("harness_tui_dual_binary_cli_pty");

    // assert
    assert!(happy_path_stage);
    assert!(dual_binary_stage);
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
        && body.contains("HARNESS_TUI_PTY_SIGNOFF=1")
        && body.contains("dual_binary_cli_pty");
    let fail_closed = !body.contains("|| true")
        && body.contains("silent skip is forbidden")
        && body.contains("pty-lane-verdict.txt");
    let lists_stages = body.contains("stages=testkit_pty,tui_pty,happy_path,dual_binary");

    // assert
    assert!(
        help_declares,
        "help/usage must document fail-closed signoff-pty"
    );
    assert!(
        owns_owners,
        "signoff-pty must gate PTY owners and dual-binary journeys"
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
