use std::path::PathBuf;

use harness_testkit::feature_simulator::{
    run_feature_simulator, write_feature_simulator_artifacts,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
        .to_path_buf()
}

#[tokio::test]
async fn feature_simulator_covers_selected_workflow_matrix_rows() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("tempdir");
    let report = run_feature_simulator(temp.path(), root.join("docs/workflow-parity-matrix.json"))
        .await
        .expect("feature simulator should produce coverage");

    assert!(report.coverage.selected_rows >= 10);
    assert_eq!(report.coverage.failed, Vec::<String>::new());
    assert!(report.deterministic_negative_paths_passed);
    assert!(report.replay_evidence_passed);
    assert!(report.replay_event_count > 0);
    for scenario in &report.scenarios {
        assert!(!scenario.case_id.trim().is_empty());
        assert!(scenario.proof_dossier_path.ends_with("/dossier.json"));
        assert!(scenario.proof_bundle_path.ends_with("/proof-bundle.json"));
        assert!(
            PathBuf::from(&scenario.proof_bundle_path).is_file(),
            "missing proof bundle for {} at {}",
            scenario.case_id,
            scenario.proof_bundle_path
        );
        assert_eq!(scenario.validation_errors, Vec::<String>::new());
        assert!(!scenario.public_surfaces.is_empty());
        if scenario.mutability == "read_expected_no_append" {
            assert_eq!(scenario.authority, "replay projection read");
            assert!(scenario.expected_event_types.is_empty());
        } else {
            assert_eq!(scenario.authority, "active workflow mutation");
            assert_eq!(scenario.mutability, "append_expected");
            assert!(scenario
                .expected_event_types
                .iter()
                .any(|event| event == "WorkflowStarted"));
        }
        assert!(!scenario.negative_fixture.trim().is_empty());
    }

    let artifact_dir = temp.path().join("feature-artifacts");
    write_feature_simulator_artifacts(&report, &artifact_dir)
        .expect("write feature simulator artifacts");
    assert!(artifact_dir.join("matrix-report.json").is_file());
    assert!(artifact_dir.join("coverage-summary.json").is_file());

    let latest_dir = root.join("target/harness-parity/latest");
    if latest_dir.exists() {
        std::fs::remove_dir_all(&latest_dir).expect("remove stale latest parity artifacts");
    }
    write_feature_simulator_artifacts(&report, &latest_dir)
        .expect("write latest strict parity proof artifacts");
    for scenario in &report.scenarios {
        let proof = latest_dir
            .join("selected-workflows")
            .join(scenario.case_id.rsplit("::").next().expect("scenario slug"))
            .join("proof-bundle.json");
        assert!(
            proof.is_file(),
            "missing mirrored proof {}",
            proof.display()
        );
    }
}

#[test]
fn simulator_fixture_declares_required_negative_contracts() {
    let root = repo_root();
    let fixture_path =
        root.join("crates/harness-testkit/fixtures/simulator/selected-workflow-scenarios.json");
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&fixture_path).expect("read simulator fixture"))
            .expect("simulator fixture json");
    let required = fixture["requiredFields"]
        .as_array()
        .expect("required fields array");
    for field in [
        "case_id",
        "workflow_or_command_id",
        "proof_dossier_path",
        "expected_event_types",
        "required_evidence_categories",
        "negative_fixture",
        "artifact_expectations",
        "proof_bundle_path",
        "validation_errors",
    ] {
        assert!(
            required.iter().any(|value| value == field),
            "simulator fixture missing required field {field}"
        );
    }
    let negatives = fixture["negativeFixtures"]
        .as_array()
        .expect("negative fixtures");
    assert!(negatives
        .iter()
        .any(|value| value == "permission denial or missing decision"));
    assert!(negatives
        .iter()
        .any(|value| value == "late completion ignored"));
}
