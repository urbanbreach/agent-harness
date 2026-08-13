#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "lane contract tests use fail-fast fixture assertions"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const RETIRED_DIGEST: &str = "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const RETIRED_REVISION_PREFIX: &str = "c1b5909ec707c069f1d21a93917af044";
const RETIRED_REVISION_SUFFIX: &str = "e71da0d7";

#[test]
fn signoff_parity_preflight_reads_active_reference_authority() {
    // Given
    let root = repo_root();
    let script = fs::read_to_string(root.join("scripts/test-lanes.sh")).expect("lane script");
    let authority: Value = serde_json::from_slice(
        &fs::read(root.join("configs/tui-fidelity-reference-authority.json"))
            .expect("reference authority"),
    )
    .expect("reference authority JSON");

    // When
    let body = function_body(&script, "run_signoff_parity");

    // Then
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
fn signoff_parity_dry_run_uses_copied_authority_mutation() {
    // Given
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

    // When
    let output = Command::new("bash")
        .arg(fixture_scripts.join("test-lanes.sh"))
        .args(["signoff-parity", "--dry-run", "--artifact-dir"])
        .arg(fixture.path().join("artifacts"))
        .output()
        .expect("dry-run lane");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Then
    assert!(output.status.success(), "dry-run failed: {stdout}");
    for (_, value) in mutated {
        assert!(
            stdout.contains(value),
            "dry-run did not consume mutated authority value {value}: {stdout}"
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
