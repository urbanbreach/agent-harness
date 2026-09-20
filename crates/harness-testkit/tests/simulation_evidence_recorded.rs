use harness_testkit::simulation::{
    build_normalized_summary, write_json_pretty, REQUIRED_ARTIFACTS,
};
use harness_testkit::UnwrapOrAbort;
use serde_json::json;
use std::fs;
use std::process::Command;

mod support;

use support::simulation_validator::{matrix, valid_matrix};

#[test]
fn evidence_publishes_only_validated_bundles_and_preserves_existing_output() {
    for scenario in [
        "secret",
        "matrix-secret",
        "escaped-matrix-secret",
        "malformed",
        "schema",
        "invariant",
        "same-seed",
        "valid",
    ] {
        for destination in ["absent", "empty", "populated"] {
            // Given: local recorded inputs and an absent, empty, or prior-evidence destination.
            let root = tempfile::tempdir().unwrap_or_abort();
            let inputs = root.path().join("inputs");
            fs::create_dir(&inputs).unwrap_or_abort();
            let artifact_root = root.path().join("artifacts");
            if destination != "absent" {
                fs::create_dir(&artifact_root).unwrap_or_abort();
            }
            if destination == "populated" {
                fs::write(
                    artifact_root.join("simulation-summary.txt"),
                    "prior evidence\n",
                )
                .unwrap_or_abort();
            }
            let secret = format!("sk-{}", "synthetic".repeat(4));
            let mut event = json!({
                "seq": 1, "run_id": "fixture",
                "payload": {"event_type": "run_started", "data": {"run_name": "fixture"}}
            });
            let replay = json!({"run_id": "fixture", "status": "completed"});
            let mut repeat_replay = replay.clone();
            let mut matrix_value = valid_matrix();
            matrix_value["scenarios"][0]["expected_predicates"] =
                build_normalized_summary(&matrix(), std::slice::from_ref(&event), &replay, "0");
            match scenario {
                "secret" => event["payload"]["data"]["run_name"] = json!(secret),
                "matrix-secret" | "escaped-matrix-secret" => {
                    matrix_value["schema_version"] = json!(secret);
                }
                "schema" => event["seq"] = json!(3),
                "invariant" => {
                    matrix_value["scenarios"][0]["expected_predicates"]["event_kind_counts"] =
                        json!({"run_started": 2});
                }
                "same-seed" => repeat_replay["status"] = json!("different"),
                _ => {}
            }
            fs::write(
                inputs.join("events.jsonl"),
                if scenario == "malformed" {
                    "{".to_owned()
                } else {
                    event.to_string()
                },
            )
            .unwrap_or_abort();
            let mut matrix_text = serde_json::to_string_pretty(&matrix_value).unwrap_or_abort();
            if scenario == "escaped-matrix-secret" {
                matrix_text = matrix_text.replace("sk-", r"\u0073k-");
            }
            fs::write(inputs.join("matrix.json"), matrix_text).unwrap_or_abort();
            write_json_pretty(&inputs.join("baseline.json"), &replay).unwrap_or_abort();
            write_json_pretty(&inputs.join("repeat.json"), &repeat_replay).unwrap_or_abort();

            // When: the built command generates the bundle through its public CLI boundary.
            let output = Command::new(env!("CARGO_BIN_EXE_simulation_evidence"))
                .arg("--artifact-root")
                .arg(&artifact_root)
                .arg("--matrix")
                .arg(inputs.join("matrix.json"))
                .arg("--baseline-events")
                .arg(inputs.join("events.jsonl"))
                .arg("--repeat-events")
                .arg(inputs.join("events.jsonl"))
                .arg("--baseline-replay")
                .arg(inputs.join("baseline.json"))
                .arg("--repeat-replay")
                .arg(inputs.join("repeat.json"))
                .output()
                .unwrap_or_abort();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!stdout.contains(&secret), "stdout exposed a secret");
            assert!(!stderr.contains(&secret), "stderr exposed a secret");

            // Then: only a successful fresh destination contains a complete bundle.
            if scenario == "valid" && destination != "populated" {
                assert!(
                    output.status.success(),
                    "{scenario}/{destination}: {stderr}"
                );
                assert!(stdout.contains("simulation evidence PASS"));
                assert!(stdout.contains(&format!("artifact_root={}", artifact_root.display())));
                for artifact in REQUIRED_ARTIFACTS {
                    assert!(artifact_root.join(artifact).is_file(), "missing {artifact}");
                }
                let index = fs::read_to_string(artifact_root.join("artifact-index.jsonl"))
                    .unwrap_or_abort();
                assert!(!index.contains(".simulation-evidence-"));
                assert!(artifact_root
                    .join("raw-evidence/baseline/events.jsonl")
                    .is_file());
            } else {
                assert!(
                    !output.status.success(),
                    "{scenario}/{destination} unexpectedly passed"
                );
                assert!(!stdout.contains("PASS"));
                match destination {
                    "absent" => assert!(
                        !artifact_root.exists(),
                        "{scenario} published rejected data"
                    ),
                    "empty" => {
                        assert_eq!(fs::read_dir(&artifact_root).unwrap_or_abort().count(), 0)
                    }
                    _ => {
                        assert_eq!(fs::read_dir(&artifact_root).unwrap_or_abort().count(), 1);
                        assert_eq!(
                            fs::read_to_string(artifact_root.join("simulation-summary.txt"))
                                .unwrap_or_abort(),
                            "prior evidence\n"
                        );
                    }
                }
            }
            assert!(
                fs::read_dir(root.path()).unwrap_or_abort().all(|entry| {
                    !entry
                        .unwrap_or_abort()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".simulation-evidence-")
                }),
                "staging directory was not cleaned up"
            );
        }
    }
}
