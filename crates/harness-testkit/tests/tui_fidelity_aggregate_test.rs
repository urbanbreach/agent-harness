#![allow(clippy::expect_used, reason = "fixture setup fails fast")]

use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity_aggregate::{aggregate, AggregateError};
use sha2::{Digest, Sha256};

#[test]
fn five_matching_fresh_runs_aggregate() {
    // Given: exactly five fresh passing run roots with one shared authority and input order.
    let fixture = AggregateFixture::new();

    // When: the typed five-run aggregator consumes the evidence.
    let summary = aggregate(&fixture.roots).expect("five-run aggregate");

    // Then: aggregate thresholds and zero-idle accounting are emitted as typed JSON data.
    assert_eq!(summary.run_count, 5);
    assert_eq!(summary.reference_external_p95_micros, 100);
    assert_eq!(summary.candidate_external_p95_micros, 105);
    assert_eq!(summary.candidate_interval_max_micros, 100);
    assert_eq!(summary.idle_redraws, 0);
    assert_eq!(summary.artifact_sha256.len(), 25);
}

#[test]
fn fewer_than_five_and_malformed_evidence_fail_closed() {
    // Given: four otherwise valid roots and one run with malformed comparison JSON.
    let fixture = AggregateFixture::new();

    // When: count and persisted JSON boundaries are evaluated.
    let count_error = aggregate(&fixture.roots[..4]).expect_err("four runs rejected");
    std::fs::write(fixture.roots[4].join("comparison.json"), b"{").expect("malform comparison");
    let malformed_error = aggregate(&fixture.roots).expect_err("malformed run rejected");

    // Then: both failures are typed and no summary is emitted.
    assert!(matches!(count_error, AggregateError::RunCount(4)));
    assert!(matches!(malformed_error, AggregateError::Evidence { .. }));
}

#[test]
fn mixed_authority_and_stale_artifact_fail_closed() {
    // Given: one candidate digest from another authority and one hash-bound artifact mutation.
    let fixture = AggregateFixture::new();
    mutate_json(&fixture.roots[4].join("receipt.json"), |value| {
        value["runtimes"][1]["binary"]["sha256"] = serde_json::json!("9".repeat(64));
    });

    // When: authority and artifact freshness are checked independently.
    let mixed_error = aggregate(&fixture.roots).expect_err("mixed authority rejected");
    let fresh_fixture = AggregateFixture::new();
    std::fs::write(fresh_fixture.roots[3].join("harness.raw"), b"tampered")
        .expect("tamper artifact");
    let stale_error = aggregate(&fresh_fixture.roots).expect_err("stale artifact rejected");

    // Then: the aggregate rejects both disconnected and stale evidence.
    assert!(matches!(mixed_error, AggregateError::MixedAuthority(_)));
    assert!(matches!(stale_error, AggregateError::Evidence { .. }));
}

struct AggregateFixture {
    _temp: tempfile::TempDir,
    roots: Vec<PathBuf>,
}

impl AggregateFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("aggregate tempdir");
        let roots = (0..5)
            .map(|ordinal| {
                let root = temp.path().join(format!("run-{ordinal}"));
                write_run(&root);
                root
            })
            .collect();
        Self { _temp: temp, roots }
    }
}

fn write_run(root: &Path) {
    std::fs::create_dir_all(root).expect("run root");
    let grok_artifacts = write_artifacts(root, "grok");
    let harness_artifacts = write_artifacts(root, "harness");
    let native_sidecar = root.join("native.json");
    std::fs::write(&native_sidecar, b"native-sidecar").expect("native sidecar");
    let binding = serde_json::json!({
        "receipt_schema":"runner.v3","scenario_id":"scenario","action_schedule_sha256":"a".repeat(64),
        "motion_contract_sha256":"m".repeat(64),"observer_version":"observer.v1",
        "terminal_identity":"xterm-256color","measurement_kind":"external_pty_observed"
    });
    let external = |artifacts: &(PathBuf, String, PathBuf, String)| {
        serde_json::json!({
            "actual_input_sends":[
                {"interaction_id":"scenario:action:0","action_ordinal":0},
                {"interaction_id":"scenario:action:1","action_ordinal":1}
            ],
            "raw_ansi":{"path":artifacts.0,"sha256":artifacts.1},
            "observations_artifact":{"path":artifacts.2,"sha256":artifacts.3}
        })
    };
    let receipt = serde_json::json!({
        "schema_version":"runner.v3","scenario_id":"scenario","runtimes":[
            {"adapter":"grok","binary":{"sha256":"1".repeat(64)},
             "presentation":{"kind":"external_only","external":external(&grok_artifacts)},
             "presentation_binding":binding},
            {"adapter":"harness","binary":{"sha256":"2".repeat(64)},
             "presentation":{"kind":"harness_native","external":external(&harness_artifacts),
                "native":{"aggregates":{"idle_redraws":0}},
                "native_trace_artifact":{"path":native_sidecar,"sha256":digest(&native_sidecar)}},
             "presentation_binding":binding}
        ]
    });
    let metrics = |latency: u64, native: bool| {
        serde_json::json!({
            "external_send_to_changed_observation_micros":[latency,latency],
            "external_observation_timestamps_micros":[1,101,201],
            "external_observation_intervals_micros":[100,100],"external_cadence_micros":100,
            "native": if native { Some(serde_json::json!({
                "receive_to_successful_flush_micros":[5],"request_to_successful_flush_micros":[4],
                "completed_write_timestamps_micros":[8],"completed_write_intervals_micros":[1],
                "coalesced_requests":0,"queue_saturation":0,"resyncs":0,"full_repaints":1,
                "bytes_written":8,"idle_redraws":0
            })) } else { None }
        })
    };
    let comparison = serde_json::json!({
        "schema_version":"comparison.v1","capture_succeeded":true,"comparison_passed":true,
        "gates":{"presentation":{"passed":true,"detail":"passed"}},
        "presentation":{"reference":metrics(100,false),"candidate":metrics(105,true)}
    });
    let cleanup = serde_json::json!({
        "schema_version":"cleanup.v1","status":"clean","forced_termination_observed":false,
        "detected_child_pids":[],"surviving_pids":[],"temporary_paths_removed":[],
        "cleanup_errors":[],"primary_error":null
    });
    write_json(&root.join("receipt.json"), &receipt);
    write_json(&root.join("comparison.json"), &comparison);
    write_json(&root.join("cleanup.json"), &cleanup);
}

fn write_artifacts(root: &Path, prefix: &str) -> (PathBuf, String, PathBuf, String) {
    let raw = root.join(format!("{prefix}.raw"));
    let observations = root.join(format!("{prefix}.observations"));
    std::fs::write(&raw, format!("{prefix}-raw")).expect("raw artifact");
    std::fs::write(&observations, format!("{prefix}-observations")).expect("observations artifact");
    (
        raw.clone(),
        digest(&raw),
        observations.clone(),
        digest(&observations),
    )
}

fn digest(path: &Path) -> String {
    Sha256::digest(std::fs::read(path).expect("read artifact"))
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        })
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize JSON"),
    )
    .expect("write JSON");
}

fn mutate_json(path: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).expect("read JSON")).expect("parse JSON");
    mutate(&mut value);
    write_json(path, &value);
}
