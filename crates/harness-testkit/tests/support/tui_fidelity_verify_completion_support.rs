use harness_testkit::tui_fidelity_verify::{
    build_plan, execute_plan, validate_active_completion, CompletionBindings, PlanSelection,
    VerificationProfile, VerifyConfig,
};

use super::fixture::synthetic_fixture;

#[test]
fn sealed_verify_all_receipt_is_the_only_active_completion_authority() {
    // arrange: a successful all-profile execution bound to the active inputs.
    let (inventory, manifest) = synthetic_fixture();
    let plan = build_plan(
        PlanSelection {
            profile: VerificationProfile::All,
            changed: None,
        },
        &inventory,
        &manifest,
    )
    .expect("all plan");
    let root = tempfile::tempdir().expect("evidence root");
    let bindings = CompletionBindings {
        candidate_sha: "f".repeat(40),
        authority_sha256: "a".repeat(64),
        inventory_sha256: "b".repeat(64),
        coverage_sha256: "c".repeat(64),
    };
    let receipt = execute_plan(
        &VerifyConfig {
            candidate_sha: bindings.candidate_sha.clone(),
            authority_sha256: bindings.authority_sha256.clone(),
            inventory_sha256: bindings.inventory_sha256.clone(),
            coverage_sha256: bindings.coverage_sha256.clone(),
            attempt_id: "active-completion".to_owned(),
            evidence_root: root.path().to_path_buf(),
            workers: Some(2),
        },
        &plan,
        |_key, isolation| {
            let artifact = isolation.evidence_dir.join("receipt.json");
            std::fs::write(&artifact, b"passed").map_err(|error| error.to_string())?;
            Ok(artifact)
        },
    )
    .expect("sealed all execution");
    let receipt_path =
        std::path::Path::new(&receipt.evidence_path).join("verification-receipt.json");

    // act: active completion is validated against the expected candidate and input digests.
    let completion =
        validate_active_completion(&receipt_path, &bindings).expect("active completion authority");

    // assert: the authority is the sealed receipt itself, with a content digest.
    assert_eq!(completion.verification_receipt_path, receipt_path);
    assert_eq!(completion.verification_receipt_sha256.len(), 64);
}
