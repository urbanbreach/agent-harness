#[test]
fn candidate_binding_accepts_complete_v2_shape() {
    // arrange: every immutable input required for an acceptance-capable candidate binding.
    let value = serde_json::json!({
        "schema_version": "harness.tui-fidelity.candidate-binding.v2",
        "receipt_kind": "release",
        "repository": {
            "canonical_path": "/repo",
            "head": "a".repeat(40),
            "tree": "b".repeat(40),
            "clean": true,
            "tracked_source_sha256": "c".repeat(64),
            "dirty_diff_sha256": "d".repeat(64),
            "untracked_manifest_sha256": "e".repeat(64),
            "cargo_lock_sha256": "f".repeat(64),
            "toolchain_sha256": "1".repeat(64),
            "cargo_config_sha256": null
        },
        "binaries": {
            "harness_sha256": "2".repeat(64),
            "runner_sha256": "3".repeat(64),
            "aggregate_sha256": "4".repeat(64)
        },
        "target_dir": "/repo/target/candidate",
        "authority": {
            "path": "/repo/configs/tui-fidelity-reference-authority.json",
            "revision": "5".repeat(40),
            "sha256": "6".repeat(64)
        },
        "reference_receipt": {
            "path": "/repo/configs/tui-fidelity-reference-binary-receipt.json",
            "sha256": "7".repeat(64)
        },
        "source_guard_receipt_sha256": "8".repeat(64),
        "parity_acceptance_eligible": true,
        "release_eligible": true,
        "clean_release": true
    });

    // act: the candidate binding crosses the JSON boundary.
    let binding =
        serde_json::from_value::<harness_testkit::tui_fidelity_runner::CandidateBinding>(value);

    // assert: the complete v2 contract parses as a typed binding.
    assert!(binding.is_ok(), "v2 binding must parse: {binding:?}");
}

#[test]
fn candidate_binding_rejects_missing_v2_field() {
    // arrange: a v2-shaped receipt with its acceptance decision omitted.
    let value = serde_json::json!({
        "schema_version": "harness.tui-fidelity.candidate-binding.v2",
        "receipt_kind": "release"
    });

    // act: the incomplete receipt crosses the JSON boundary.
    let binding =
        serde_json::from_value::<harness_testkit::tui_fidelity_runner::CandidateBinding>(value);

    // assert: missing provenance is rejected instead of defaulted.
    assert!(binding.is_err());
}
