#![allow(clippy::expect_used, reason = "contract fixtures fail fast")]

use std::fs;

use harness_testkit::tui_fidelity::{AdapterKind, Scenario, ScenarioAction};
use harness_testkit::tui_fidelity_packet6::build_capability_receipt;

const COMPOSER: &str = include_str!("../src/tui_fidelity_scenarios/baseline/packet6-composer.json");

#[test]
fn composer_journey_uses_identical_natural_input_for_both_adapters() {
    // arrange
    // act
    let scenario = Scenario::from_json(COMPOSER).expect("Packet 6 composer scenario");

    // assert
    assert!(scenario.validate_for_adapter(AdapterKind::Grok).is_ok());
    assert!(scenario.validate_for_adapter(AdapterKind::Harness).is_ok());
    assert_eq!(
        scenario.adapters,
        vec![AdapterKind::Grok, AdapterKind::Harness]
    );
    assert!(matches!(
        scenario.actions.as_slice(),
        [ScenarioAction::TypeText(typed), ScenarioAction::WaitForText(wait)]
            if wait.text == "中🙂"
                && typed.text.contains(&wait.text)
                && typed.text.starts_with("Draft a concise release note")
    ));
}

#[test]
fn capability_receipt_labels_unavailable_process_variants_without_parity() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let input = capability_input(temp.path(), false);

    let receipt = build_capability_receipt(&input, temp.path(), "a".repeat(64).as_str())
        .expect("unsupported capability receipt");
    // act
    let value: serde_json::Value = serde_json::from_str(&receipt).expect("receipt JSON");

    // assert
    assert_eq!(value["comparison_claimed"], false);
    assert_eq!(value["rows"][0]["status"], "supported_by_both");
    assert_eq!(value["rows"][5]["status"], "harness_only");
}

#[test]
fn capability_receipt_fails_closed_on_missing_or_forged_process_proof() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let mut input: serde_json::Value =
        serde_json::from_str(&capability_input(temp.path(), true)).expect("input JSON");
    input["rows"][0]["reference"]["evidence_sha256"] = serde_json::json!("0".repeat(64));

    // act
    let error = build_capability_receipt(
        &serde_json::to_string(&input).expect("mutation JSON"),
        temp.path(),
        "a".repeat(64).as_str(),
    )
    .expect_err("forged proof must fail");

    // assert
    assert!(error.to_string().contains("digest"), "{error}");
}

#[test]
fn capability_receipt_rejects_duplicate_variant_with_missing_peer() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let mut input: serde_json::Value =
        serde_json::from_str(&capability_input(temp.path(), false)).expect("input JSON");
    input["rows"][6]["capability"] = serde_json::json!("truecolor");

    // act
    let error = build_capability_receipt(
        &serde_json::to_string(&input).expect("mutation JSON"),
        temp.path(),
        "a".repeat(64).as_str(),
    )
    .expect_err("duplicate capability must fail");

    // assert
    assert!(error.to_string().contains("seven unique"), "{error}");
}

#[test]
fn capability_receipt_rejects_non_authority_digest() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let input = capability_input(temp.path(), false);

    // act
    let error = build_capability_receipt(&input, temp.path(), "b".repeat(64).as_str())
        .expect_err("retired authority digest must fail");

    // assert
    assert!(error.to_string().contains("authority binary"), "{error}");
}

#[cfg(unix)]
#[test]
fn capability_receipt_rejects_symlink_escape() {
    // arrange
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::NamedTempFile::new().expect("outside proof");
    fs::write(outside.path(), b"{}\n").expect("outside bytes");
    symlink(outside.path(), temp.path().join("process-proof.json")).expect("proof symlink");
    let input = capability_input_for_existing_proof(temp.path(), false);

    // act
    let error = build_capability_receipt(&input, temp.path(), "a".repeat(64).as_str())
        .expect_err("symlink escape must fail");

    // assert
    assert!(
        error.to_string().contains("escapes evidence root"),
        "{error}"
    );
}

fn capability_input(root: &std::path::Path, tmux_available: bool) -> String {
    let proof = root.join("process-proof.json");
    fs::write(&proof, b"{}\n").expect("proof");
    capability_input_for_existing_proof(root, tmux_available)
}

fn capability_input_for_existing_proof(root: &std::path::Path, tmux_available: bool) -> String {
    let proof = root.join("process-proof.json");
    let digest = sha256(&fs::read(&proof).expect("proof bytes"));
    let available = serde_json::json!({
        "availability": "available",
        "evidence_path": "process-proof.json",
        "evidence_sha256": digest,
        "observable": "real process probe exited zero"
    });
    let unavailable = serde_json::json!({
        "availability": "unavailable",
        "observable": "tmux and SSH process unavailable"
    });
    let capabilities = [
        "truecolor",
        "indexed_256",
        "ansi_16",
        "reduced_motion",
        "legacy_keys",
        "tmux_ssh",
        "cjk_emoji_wide",
    ];
    let rows = capabilities
        .into_iter()
        .map(|capability| {
            let reference = if capability == "tmux_ssh" && !tmux_available {
                unavailable.clone()
            } else {
                available.clone()
            };
            serde_json::json!({
                "capability": capability,
                "reference": reference,
                "harness": available
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": "harness.tui-fidelity.packet6-capability-input.v1",
        "authority_binary_sha256": "a".repeat(64),
        "rows": rows
    })
    .to_string()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    use std::fmt::Write as _;

    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("write digest");
            output
        })
}
