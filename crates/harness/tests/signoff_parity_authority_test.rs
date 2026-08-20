#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "lane contract tests use fail-fast fixture assertions"
)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

const RETIRED_DIGEST: &str = "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const RETIRED_REVISION_PREFIX: &str = "c1b5909ec707c069f1d21a93917af044";
const RETIRED_REVISION_SUFFIX: &str = "e71da0d7";

#[test]
fn signoff_parity_preflight_reads_active_reference_authority() {
    // arrange
    let root = repo_root();
    let script = fs::read_to_string(root.join("scripts/test-lanes.sh")).expect("lane script");
    let authority: Value = serde_json::from_slice(
        &fs::read(root.join("configs/tui-fidelity-reference-authority.json"))
            .expect("reference authority"),
    )
    .expect("reference authority JSON");

    // act
    let body = function_body(&script, "run_signoff_parity");

    // assert
    assert!(body.contains("configs/tui-fidelity-reference-authority.json"));
    for field in [
        "executable",
        "canonical_checkout",
        "source_revision",
        "binary_sha256",
        "binary_version",
    ] {
        assert!(
            body.contains(field),
            "lane does not read authority field {field}"
        );
        assert!(authority["reference"][field].is_string());
    }
    assert!(!body.contains(RETIRED_DIGEST));
    assert!(!body.contains(&format!(
        "{RETIRED_REVISION_PREFIX}{RETIRED_REVISION_SUFFIX}"
    )));
    for epoch in authority["prior_binary_epochs"]
        .as_array()
        .expect("prior binary epochs")
    {
        let digest = epoch["binary_sha256"]
            .as_str()
            .expect("prior binary digest");
        assert!(!body.contains(digest));
    }
}

#[test]
fn historical_manifest_is_never_completion_authority() {
    // arrange: the canonical lane script.
    let script =
        fs::read_to_string(repo_root().join("scripts/test-lanes.sh")).expect("lane script");

    // act: the final verdict writer is isolated from evidence-generation stages.
    let body = function_body(&script, "write_signoff_parity_verdict");

    // assert: no frozen historical manifest field can derive parity completion.
    assert!(!body.contains("tui-reference-parity-manifest.v1.json"));
    assert!(!body.contains("manifest_sha256"));
    assert!(!body.contains("rows"));
}

#[test]
fn signoff_parity_authority_bindings_use_copied_mutation() {
    // arrange
    let fixture = tempfile::tempdir().expect("temporary lane fixture");
    let fixture_scripts = fixture.path().join("scripts");
    let fixture_configs = fixture.path().join("configs");
    fs::create_dir_all(&fixture_scripts).expect("fixture scripts directory");
    fs::create_dir_all(&fixture_configs).expect("fixture configs directory");
    fs::copy(
        repo_root().join("scripts/test-lanes.sh"),
        fixture_scripts.join("test-lanes.sh"),
    )
    .expect("copied lane script");

    let mut authority: Value = serde_json::from_slice(
        &fs::read(repo_root().join("configs/tui-fidelity-reference-authority.json"))
            .expect("reference authority"),
    )
    .expect("reference authority JSON");
    let mutated = [
        ("executable", "reference/bin"),
        ("canonical_checkout", "reference/checkout"),
        (
            "source_revision",
            "1111111111111111111111111111111111111111",
        ),
        (
            "binary_sha256",
            "2222222222222222222222222222222222222222222222222222222222222222",
        ),
        ("binary_version", "reference-version"),
    ];
    for (field, value) in mutated {
        authority["reference"][field] = Value::String(value.to_owned());
    }
    fs::write(
        fixture_configs.join("tui-fidelity-reference-authority.json"),
        serde_json::to_vec(&authority).expect("serialized authority"),
    )
    .expect("copied authority");

    // act
    let copied: Value = serde_json::from_slice(
        &fs::read(fixture_configs.join("tui-fidelity-reference-authority.json"))
            .expect("copied authority"),
    )
    .expect("copied authority JSON");
    let script =
        fs::read_to_string(fixture_scripts.join("test-lanes.sh")).expect("copied lane script");
    let body = function_body(&script, "run_signoff_parity");

    // assert
    for (index, (field, value)) in mutated.into_iter().enumerate() {
        assert_eq!(copied["reference"][field], value);
        assert!(
            body.contains(&format!("reference_authority_fields[{index}]")),
            "lane does not bind copied authority field {field}"
        );
    }
}

fn function_body<'a>(script: &'a str, name: &str) -> &'a str {
    let start = script
        .find(&format!("{name}() {{"))
        .unwrap_or_else(|| panic!("missing function {name}"));
    let remainder = &script[start..];
    let end = remainder[1..]
        .find("\n}\n")
        .map(|offset| offset + 1)
        .unwrap_or(remainder.len());
    &remainder[..end]
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
