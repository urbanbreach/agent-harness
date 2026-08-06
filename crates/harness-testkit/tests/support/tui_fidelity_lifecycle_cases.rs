use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use harness_testkit::tui_fidelity::Scenario;
use harness_testkit::tui_fidelity_runner::run_compare;

use super::support::{Fixture, STARTUP_SMOKE};

#[test]
fn preflight_failures_write_cleanup_receipts() {
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let mut browser = Fixture::new("normal", "normal", "normal");
    browser.config.renderer.browser_program = browser.root().join("missing-browser");
    let mut font = Fixture::new("normal", "normal", "normal");
    font.config.renderer.font_family = "Definitely Missing Lifecycle Font".to_owned();

    run_compare(&scenario, &browser.config).expect_err("missing browser");
    run_compare(&scenario, &font.config).expect_err("missing font");

    assert!(browser.config.evidence_dir.join("cleanup.json").is_file());
    assert!(font.config.evidence_dir.join("cleanup.json").is_file());
}

#[test]
fn stale_evidence_is_preserved_and_receives_attempt_cleanup() {
    let fixture = Fixture::new("normal", "normal", "normal");
    fs::create_dir_all(&fixture.config.evidence_dir).expect("evidence dir");
    let sentinel = fixture.config.evidence_dir.join("sentinel.txt");
    fs::write(&sentinel, "owned").expect("sentinel");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    run_compare(&scenario, &fixture.config).expect_err("stale evidence");

    let cleanup_attempt = fs::read_dir(&fixture.config.evidence_dir)
        .expect("read evidence")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("cleanup-attempt-")
        });
    assert!(sentinel.is_file());
    assert!(cleanup_attempt, "stale failure needs append-only cleanup");
}

#[test]
fn preexisting_runtime_root_and_sentinel_are_preserved() {
    let fixture = Fixture::new("normal", "normal", "normal");
    let sentinel = fixture.root().join("tmp/tui-fidelity/sentinel.txt");
    fs::create_dir_all(sentinel.parent().expect("sentinel parent")).expect("runtime base");
    fs::write(&sentinel, "owned").expect("sentinel");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    run_compare(&scenario, &fixture.config).expect("normal compare");

    assert_eq!(
        fs::read_to_string(sentinel).expect("preserved sentinel"),
        "owned"
    );
}

#[test]
fn cleanup_failure_retains_primary_error_and_writes_receipt() {
    let fixture = Fixture::new("cleanup-failure", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    let error = run_compare(&scenario, &fixture.config).expect_err("cleanup must fail");

    let message = error.to_string();
    assert!(message.contains("primary") && message.contains("cleanup"));
    assert!(fixture.config.evidence_dir.join("cleanup.json").is_file());
    let sabotaged = fixture.root().join("tmp/tui-fidelity");
    if sabotaged.is_file() {
        fs::remove_file(sabotaged).expect("remove sabotage file");
    }
}

#[test]
fn detected_child_pids_are_recorded_separately_from_survivors() {
    let fixture = Fixture::new("survivor", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    run_compare(&scenario, &fixture.config).expect_err("survivor must fail");

    let cleanup: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.config.evidence_dir.join("cleanup.json")).expect("cleanup receipt"),
    )
    .expect("cleanup json");
    let detected = cleanup["detected_child_pids"]
        .as_array()
        .expect("detected child pids");
    let surviving = cleanup["surviving_pids"]
        .as_array()
        .expect("surviving pids");
    assert!(
        !detected.is_empty(),
        "unexpected child PIDs must be recorded"
    );
    assert!(
        surviving.is_empty(),
        "successfully reaped PIDs did not survive"
    );
    assert!(detected.iter().all(|pid| {
        !std::path::Path::new(&format!("/proc/{}", pid.as_u64().expect("pid"))).exists()
    }));
}

#[test]
fn hanging_renderer_times_out_and_is_reaped_repeatedly() {
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    for _ in 0..3 {
        let mut fixture = Fixture::new("normal", "normal", "hang");
        fixture.config.timing.cleanup_timeout = Duration::from_millis(100);

        let error = run_compare(&scenario, &fixture.config).expect_err("renderer timeout");

        assert!(error.to_string().contains("renderer timed out"));
        let pid_path = fixture.config.evidence_dir.join("grok/rest/renderer.pid");
        let pid = fs::read_to_string(pid_path).expect("renderer pid");
        assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
    }
}

#[test]
fn cli_preflight_failures_report_typed_errors_and_write_cleanup_receipts() {
    let fixture = Fixture::new("normal", "normal", "normal");
    let unknown_evidence = fixture.root().join("unknown-evidence");
    let missing_evidence = fixture.root().join("missing-reference-evidence");
    let missing = fixture.root().join("missing-reference");

    let unknown = run_cli(
        &fixture,
        "unknown",
        &fixture.config.reference.path,
        &unknown_evidence,
    );
    let missing = run_cli(&fixture, "startup-smoke", &missing, &missing_evidence);

    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(!unknown.status.success());
    assert!(unknown_evidence.join("cleanup.json").is_file());
    assert!(!missing.status.success());
    assert!(
        missing_stderr.contains("grok binary is missing"),
        "stderr: {missing_stderr}"
    );
    assert!(missing_evidence.join("cleanup.json").is_file());
}

fn run_cli(fixture: &Fixture, scenario: &str, reference: &Path, evidence: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tui-fidelity"))
        .args(["compare", "--scenario", scenario, "--reference-bin"])
        .arg(reference)
        .arg("--harness-bin")
        .arg(&fixture.config.harness.path)
        .arg("--candidate-receipt")
        .arg(fixture.root().join("candidate-receipt.json"))
        .arg("--evidence-dir")
        .arg(evidence)
        .output()
        .expect("run CLI")
}
