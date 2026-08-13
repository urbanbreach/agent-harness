#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "fixture setup fails fast"
)]

use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity_aggregate::{aggregate, aggregate_with_profile, AggregateError};
use harness_testkit::tui_fidelity_compare::AcceptanceProfile;
use sha2::{Digest, Sha256};

#[path = "support/tui_fidelity_no_visible_gap.rs"]
mod tui_fidelity_no_visible_gap;

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

#[test]
fn packet2_profile_keeps_visual_failures_diagnostic() {
    let fixture = AggregateFixture::new_packet2();

    let summary = aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
        .expect("scheduling profile passes");
    let full = aggregate(&fixture.roots).expect_err("full parity rejects the same visual defects");

    assert_eq!(summary.run_count, 5);
    assert!(matches!(full, AggregateError::Evidence { .. }));
    let comparison: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.roots[0].join("comparison.json")).expect("comparison"),
    )
    .expect("comparison JSON");
    assert_eq!(comparison["gates"]["semantic_cell"]["passed"], false);
    assert_eq!(comparison["gates"]["pixel"]["passed"], false);
}

#[test]
fn packet2_profile_rejects_missing_digest_and_reordered_input() {
    let missing = AggregateFixture::new_packet2();
    mutate_json(&missing.roots[0].join("receipt.json"), |value| {
        value["runtimes"][1]["presentation"]
            .as_object_mut()
            .expect("presentation")
            .remove("scheduling_sidecar");
    });
    let missing_error =
        aggregate_with_profile(&missing.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("missing digest rejected");

    let reordered = AggregateFixture::new_packet2();
    mutate_json(&reordered.roots[0].join("scheduling.json"), |value| {
        value["actual_input_sends"]
            .as_array_mut()
            .expect("inputs")
            .reverse();
    });
    mutate_json(&reordered.roots[0].join("receipt.json"), |value| {
        value["runtimes"][1]["presentation"]["scheduling_sidecar"]["sha256"] =
            serde_json::json!(digest(&reordered.roots[0].join("scheduling.json")));
    });
    let reordered_error =
        aggregate_with_profile(&reordered.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("reordered input rejected");

    let forged = AggregateFixture::new_packet2();
    mutate_json(&forged.roots[0].join("scheduling.json"), |value| {
        value["maximum_backlog_depth"] = serde_json::json!(99);
    });
    let forged_error = aggregate_with_profile(&forged.roots, AcceptanceProfile::Packet2Scheduling)
        .expect_err("forged sidecar digest rejected");

    for forged_maximum in [1, 3] {
        let mismatch = AggregateFixture::new_packet2();
        mutate_sidecar(&mismatch.roots[0], |value| {
            value["maximum_backlog_depth"] = serde_json::json!(forged_maximum);
        });
        assert!(
            aggregate_with_profile(&mismatch.roots, AcceptanceProfile::Packet2Scheduling)
                .expect_err("maximum backlog mismatch rejected")
                .to_string()
                .contains("maximum backlog differs")
        );
    }

    let synthesized = AggregateFixture::new_packet2();
    mutate_sidecar(&synthesized.roots[0], |value| {
        let send = &mut value["actual_input_sends"][0];
        send["live_ready_depth"] = serde_json::json!(1);
        send["queued_live_depth"] = serde_json::json!(0);
        send["deferred_live_ready"] = serde_json::json!(false);
        send["stream_active"] = serde_json::json!(true);
        send["preempted_live"] = serde_json::json!(true);
        value["maximum_backlog_depth"] = serde_json::json!(1);
    });
    let synthesized_error =
        aggregate_with_profile(&synthesized.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("stream-active-only backlog rejected");

    assert!(missing_error
        .to_string()
        .contains("missing Harness scheduling sidecar digest"));
    assert!(reordered_error.to_string().contains("interaction 0"));
    assert!(forged_error.to_string().contains("stale artifact digest"));
    assert!(synthesized_error.to_string().contains("relabeled"));
}

#[test]
fn packet2_profile_recomputes_required_gate_verdict_and_rejects_gate_shape_drift() {
    // Given: four receipts whose persisted top-level pass boolean remains true.
    let missing = AggregateFixture::new_packet2();
    mutate_json(&missing.roots[0].join("comparison.json"), |value| {
        value["gates"]
            .as_object_mut()
            .expect("gate map")
            .remove("timing");
    });
    let failed = AggregateFixture::new_packet2();
    mutate_json(&failed.roots[0].join("comparison.json"), |value| {
        value["gates"]["presentation"]["passed"] = serde_json::json!(false);
    });
    let extra = AggregateFixture::new_packet2();
    mutate_json(&extra.roots[0].join("comparison.json"), |value| {
        value["gates"]["invented"] = serde_json::json!({"passed":true,"detail":"forged"});
    });
    let duplicate = AggregateFixture::new_packet2();
    let duplicate_path = duplicate.roots[0].join("comparison.json");
    let text = std::fs::read_to_string(&duplicate_path).expect("comparison text");
    let text = text.replacen(
        "\"timing\": {",
        "\"timing\": {\"passed\":true,\"detail\":\"forged\"},\"timing\": {",
        1,
    );
    std::fs::write(&duplicate_path, text).expect("duplicate gate fixture");

    // When: the Packet 2 aggregate validates each persisted run.
    let results = [missing, failed, extra, duplicate]
        .into_iter()
        .map(|fixture| aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling))
        .collect::<Vec<_>>();

    // Then: missing, failed, extra, and duplicate gate evidence all fail closed.
    assert!(results.iter().all(Result::is_err));
}

#[test]
fn packet2_profile_rejects_run_count_timing_idle_backlog_and_native_proof_defects() {
    let six = AggregateFixture::new_packet2();
    let mut six_roots = six.roots.clone();
    six_roots.push(six.roots[0].clone());
    assert!(matches!(
        aggregate_with_profile(&six_roots, AcceptanceProfile::Packet2Scheduling),
        Err(AggregateError::RunCount(6))
    ));

    let p95 = AggregateFixture::new_packet2();
    mutate_json(&p95.roots[0].join("comparison.json"), |value| {
        value["presentation"]["candidate"]["external_send_to_changed_observation_micros"] =
            serde_json::json!([111, 111]);
    });
    assert!(
        aggregate_with_profile(&p95.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("111% rejected")
            .to_string()
            .contains("110%")
    );

    let gap = AggregateFixture::new_packet2();
    mutate_json(&gap.roots[0].join("comparison.json"), |value| {
        value["presentation"]["candidate"]["external_cadence_micros"] = serde_json::json!(16);
        value["presentation"]["candidate"]["external_observation_intervals_micros"] =
            serde_json::json!([33]);
    });
    assert!(
        aggregate_with_profile(&gap.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("33ms gap rejected")
            .to_string()
            .contains("twice cadence")
    );

    let semantic_boundary = AggregateFixture::new_packet2();
    configure_packet2_semantic_gap(&semantic_boundary, 66_000);
    aggregate_with_profile(
        &semantic_boundary.roots,
        AcceptanceProfile::Packet2Scheduling,
    )
    .expect("66ms semantic streaming gap accepted");
    let handshake_wait = AggregateFixture::new_packet2();
    configure_packet2_handshake_gap(&handshake_wait, 100_000, 20_000);
    aggregate_with_profile(&handshake_wait.roots, AcceptanceProfile::Packet2Scheduling)
        .expect("pre-input handshake wait with fast response accepted");
    let response_defect = AggregateFixture::new_packet2();
    configure_packet2_handshake_gap(&response_defect, 100_000, 32_001);
    assert!(
        aggregate_with_profile(&response_defect.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("slow post-send response rejected")
            .to_string()
            .contains("16 ms cadence")
    );
    let semantic_defect = AggregateFixture::new_packet2();
    configure_packet2_semantic_gap(&semantic_defect, 66_001);
    assert!(
        aggregate_with_profile(&semantic_defect.roots, AcceptanceProfile::Packet2Scheduling,)
            .expect_err("67ms semantic streaming gap rejected")
            .to_string()
            .contains("33 ms cadence")
    );

    for (field, value, expected) in [
        ("idle_redraws", 1, "zero idle redraws"),
        ("maximum_backlog_depth", 0, "maximum backlog differs"),
    ] {
        let fixture = AggregateFixture::new_packet2();
        if field == "idle_redraws" {
            mutate_json(&fixture.roots[0].join("receipt.json"), |receipt| {
                receipt["runtimes"][1]["presentation"]["native"]["aggregates"][field] =
                    serde_json::json!(value);
            });
        } else {
            mutate_sidecar(&fixture.roots[0], |sidecar| {
                sidecar[field] = serde_json::json!(value);
            });
        }
        assert!(
            aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling)
                .expect_err("controlled defect rejected")
                .to_string()
                .contains(expected)
        );
    }

    let native = AggregateFixture::new_packet2();
    mutate_json(&native.roots[0].join("receipt.json"), |receipt| {
        receipt["runtimes"][1]["presentation"]["native"]["acknowledgements"] =
            serde_json::json!([{"outcome":"failed_write"}]);
    });
    assert!(
        aggregate_with_profile(&native.roots, AcceptanceProfile::Packet2Scheduling)
            .expect_err("completed write proof required")
            .to_string()
            .contains("completed_write")
    );
}

fn configure_packet2_handshake_gap(
    fixture: &AggregateFixture,
    pre_send_micros: u64,
    post_send_micros: u64,
) {
    for root in &fixture.roots {
        mutate_json(&root.join("receipt.json"), |receipt| {
            receipt["scenario_id"] = serde_json::json!("packet2-sustained-stream");
            for runtime in receipt["runtimes"].as_array_mut().expect("runtime array") {
                runtime["presentation_binding"]["scenario_id"] =
                    serde_json::json!("packet2-sustained-stream");
                let sends = runtime["presentation"]["external"]["actual_input_sends"]
                    .as_array_mut()
                    .expect("input sends");
                sends[0]["sent_at"] = serde_json::json!(pre_send_micros);
                sends[1]["sent_at"] = serde_json::json!(200_000);
            }
        });
        mutate_json(&root.join("comparison.json"), |comparison| {
            comparison["presentation"]["candidate"]["external_observation_timestamps_micros"] =
                serde_json::json!([pre_send_micros, pre_send_micros + post_send_micros, 200_000]);
        });
    }
}

fn configure_packet2_semantic_gap(fixture: &AggregateFixture, gap_micros: u64) {
    for root in &fixture.roots {
        mutate_json(&root.join("receipt.json"), |receipt| {
            receipt["scenario_id"] = serde_json::json!("packet2-sustained-stream");
            for runtime in receipt["runtimes"].as_array_mut().expect("runtime array") {
                runtime["presentation_binding"]["scenario_id"] =
                    serde_json::json!("packet2-sustained-stream");
                let sends = runtime["presentation"]["external"]["actual_input_sends"]
                    .as_array_mut()
                    .expect("input sends");
                sends[0]["sent_at"] = serde_json::json!(0);
                sends[1]["sent_at"] = serde_json::json!(100_000);
            }
        });
        mutate_json(&root.join("comparison.json"), |comparison| {
            comparison["presentation"]["candidate"]["external_observation_timestamps_micros"] =
                serde_json::json!([1, 1 + gap_micros, 100_000]);
        });
    }
}

#[test]
fn packet2_profile_rejects_every_mixed_authority_binding() {
    for field in [
        "scenario_id",
        "receipt_schema",
        "comparison_schema",
        "reference_sha256",
        "candidate_sha256",
        "action_schedule_sha256",
        "motion_contract_sha256",
        "observer_version",
        "terminal_identity",
    ] {
        let fixture = AggregateFixture::new_packet2();
        mutate_authority(&fixture.roots[4], field);
        assert!(
            matches!(
                aggregate_with_profile(&fixture.roots, AcceptanceProfile::Packet2Scheduling),
                Err(AggregateError::MixedAuthority(_))
            ),
            "mixed field {field} must fail"
        );
    }
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

    fn new_packet2() -> Self {
        let fixture = Self::new();
        for root in &fixture.roots {
            let scheduling = root.join("scheduling.json");
            write_json(
                &scheduling,
                &serde_json::json!({
                    "schema_version":"harness.packet2-scheduling.v1",
                    "actual_input_sends":[
                        {"interaction_id":"scenario:action:0","action_ordinal":0,"terminal_sequence":1,"source_decision":"terminal_input","live_ready_depth":2,"queued_live_depth":2,"deferred_live_ready":false,"stream_active":true,"preempted_live":true,"fairness_yield":false,"deadline_millis":16,"cause_id":"cause:1"},
                        {"interaction_id":"scenario:action:1","action_ordinal":1,"terminal_sequence":2,"source_decision":"terminal_input","live_ready_depth":1,"queued_live_depth":0,"deferred_live_ready":true,"stream_active":true,"preempted_live":true,"fairness_yield":true,"deadline_millis":16,"cause_id":"cause:2"}
                    ],
                "maximum_backlog_depth":2
                }),
            );
            mutate_json(&root.join("receipt.json"), |value| {
                value["runtimes"][1]["presentation"]["scheduling_sidecar"] = serde_json::json!({
                    "path":scheduling,"sha256":digest(&scheduling)
                });
                for runtime in value["runtimes"].as_array_mut().expect("runtimes") {
                    let sends = runtime["presentation"]["external"]["actual_input_sends"]
                        .as_array_mut()
                        .expect("sends");
                    sends[0]["sent_at"] = serde_json::json!(1);
                    sends[1]["sent_at"] = serde_json::json!(201);
                }
            });
            mutate_json(&root.join("comparison.json"), |value| {
                value["acceptance_profile"] = serde_json::json!("packet2_scheduling");
                value["gates"] = serde_json::json!({
                    "presentation":{"passed":true,"detail":"passed"},
                    "timing":{"passed":true,"detail":"passed"},
                    "provenance":{"passed":true,"detail":"passed"},
                    "checkpoint":{"passed":true,"detail":"passed"},
                    "exit":{"passed":true,"detail":"passed"},
                    "cleanup":{"passed":true,"detail":"passed"},
                    "semantic_cell":{"passed":false,"detail":"diagnostic"},
                    "pixel":{"passed":false,"detail":"diagnostic"},
                    "motion":{"passed":false,"detail":"diagnostic"}
                });
            });
        }
        fixture
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
                "native":{"aggregates":{"idle_redraws":0},"acknowledgements":[{"outcome":"completed_write"}],
                    "causes":[
                        {"interaction_id":"scenario:action:0","resulting_revision":1,"outcome":"visible_change"},
                        {"interaction_id":"scenario:action:1","resulting_revision":2,"outcome":"visible_change"}
                    ]},
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
        "schema_version":"comparison.v1","acceptance_profile":"full_parity","capture_succeeded":true,"comparison_passed":true,
        "gates":{
            "presentation":{"passed":true,"detail":"passed"},
            "semantic_cell":{"passed":true,"detail":"passed"},
            "pixel":{"passed":true,"detail":"passed"},
            "motion":{"passed":true,"detail":"passed"},
            "timing":{"passed":true,"detail":"passed"},
            "provenance":{"passed":true,"detail":"passed"},
            "checkpoint":{"passed":true,"detail":"passed"},
            "exit":{"passed":true,"detail":"passed"},
            "cleanup":{"passed":true,"detail":"passed"}
        },
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

fn mutate_sidecar(root: &Path, mutate: impl FnOnce(&mut serde_json::Value)) {
    let path = root.join("scheduling.json");
    mutate_json(&path, mutate);
    mutate_json(&root.join("receipt.json"), |receipt| {
        receipt["runtimes"][1]["presentation"]["scheduling_sidecar"]["sha256"] =
            serde_json::json!(digest(&path));
    });
}

fn mutate_authority(root: &Path, field: &str) {
    mutate_json(&root.join("receipt.json"), |receipt| match field {
        "scenario_id" => {
            receipt["scenario_id"] = serde_json::json!("changed-scenario");
            for runtime in receipt["runtimes"].as_array_mut().expect("runtimes") {
                runtime["presentation_binding"]["scenario_id"] =
                    serde_json::json!("changed-scenario");
            }
        }
        "receipt_schema" => {
            receipt["schema_version"] = serde_json::json!("changed-runner");
            for runtime in receipt["runtimes"].as_array_mut().expect("runtimes") {
                runtime["presentation_binding"]["receipt_schema"] =
                    serde_json::json!("changed-runner");
            }
        }
        "reference_sha256" => {
            receipt["runtimes"][0]["binary"]["sha256"] = serde_json::json!("3".repeat(64));
        }
        "candidate_sha256" => {
            receipt["runtimes"][1]["binary"]["sha256"] = serde_json::json!("4".repeat(64));
        }
        "action_schedule_sha256"
        | "motion_contract_sha256"
        | "observer_version"
        | "terminal_identity" => {
            for runtime in receipt["runtimes"].as_array_mut().expect("runtimes") {
                runtime["presentation_binding"][field] =
                    serde_json::json!(format!("changed-{field}"));
            }
        }
        "comparison_schema" => {}
        other => panic!("unknown authority field {other}"),
    });
    if field == "comparison_schema" {
        mutate_json(&root.join("comparison.json"), |comparison| {
            comparison["schema_version"] = serde_json::json!("changed-comparison");
        });
    }
}
