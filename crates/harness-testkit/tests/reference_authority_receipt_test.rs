#![allow(clippy::expect_used, reason = "test fixture setup fails fast")]

use harness_testkit::reference_authority_receipt::ReferenceAuthorityReceipt;
use sha2::{Digest, Sha256};

#[path = "support/harness_bin.rs"]
mod harness_bin;

#[test]
fn compare_accepts_active_reference_receipt_before_candidate_preflight() {
    // arrange: the active Packet 0 receipt and pinned binary, but no candidate receipt.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let evidence = tempfile::tempdir().expect("evidence tempdir");

    // act: compare advances through reference authority validation.
    let output = harness_bin::tui_fidelity_command()
        .args(["compare", "--scenario", "startup-smoke", "--reference-bin"])
        .arg(root.join("inspirations/grok-build/target/debug/xai-grok-pager"))
        .arg("--reference-receipt")
        .arg(root.join("configs/tui-fidelity-reference-binary-receipt.json"))
        .arg("--reference-authority")
        .arg(root.join("configs/tui-fidelity-reference-authority.json"))
        .arg("--reference-root")
        .arg(root.join("inspirations/grok-build"))
        .arg("--harness-bin")
        .arg(root.join("target/missing-harness"))
        .arg("--candidate-receipt")
        .arg(root.join("target/missing-candidate-receipt.json"))
        .arg("--evidence-dir")
        .arg(evidence.path())
        .output()
        .expect("run compare preflight");
    let stderr = String::from_utf8_lossy(&output.stderr);

    // assert: failure is at the intentionally missing candidate, not the active receipt schema.
    assert!(!output.status.success());
    assert!(!stderr.contains("unknown field `observed_at`"), "{stderr}");
    assert!(
        stderr.contains("missing-candidate-receipt.json"),
        "{stderr}"
    );
}

#[test]
fn reference_receipt_mutations_fail_closed() {
    #[derive(Clone, Copy, Debug)]
    enum Mutation {
        Unknown,
        Revision,
        BinaryPath,
        BinaryDigest,
        BinaryVersion,
        TreeFormat,
        ToolchainDigestFormat,
    }

    // arrange: independently forged authority fields derived from the active receipt.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let active_path = root.join("configs/tui-fidelity-reference-binary-receipt.json");
    let active: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&active_path).expect("read active receipt"))
            .expect("parse active receipt fixture");
    let active_revision = active["source"]["revision"]
        .as_str()
        .expect("active source revision");
    let cases = [
        Mutation::Unknown,
        Mutation::Revision,
        Mutation::BinaryPath,
        Mutation::BinaryDigest,
        Mutation::BinaryVersion,
        Mutation::TreeFormat,
        Mutation::ToolchainDigestFormat,
    ];

    for mutation in cases {
        let temp = tempfile::tempdir().expect("mutation tempdir");
        let path = temp.path().join("receipt.json");
        let mut value = active.clone();
        match mutation {
            Mutation::Unknown => value["invented"] = serde_json::json!(true),
            Mutation::Revision => value["source"]["revision"] = serde_json::json!("forged"),
            Mutation::BinaryPath => value["binary"]["path"] = serde_json::json!("Cargo.toml"),
            Mutation::BinaryDigest => {
                value["binary"]["sha256"] = serde_json::json!("0".repeat(64));
            }
            Mutation::BinaryVersion => value["binary"]["version"] = serde_json::json!("forged"),
            Mutation::TreeFormat => value["source"]["tree"] = serde_json::json!("not-a-tree"),
            Mutation::ToolchainDigestFormat => {
                value["toolchain"]["rustc_sha256"] = serde_json::json!("abc");
            }
        }
        std::fs::write(
            &path,
            serde_json::to_vec(&value).expect("serialize mutation"),
        )
        .expect("write mutation");

        // act: the typed boundary reads and verifies the forged receipt.
        let result = ReferenceAuthorityReceipt::read(&path).and_then(|receipt| {
            receipt.verify(
                &root,
                &root.join("inspirations/grok-build/target/debug/xai-grok-pager"),
                active_revision,
            )
        });

        // assert: every mutation is rejected without a compatibility fallback.
        assert!(result.is_err(), "mutation {mutation:?} must fail closed");
    }
}

#[cfg(unix)]
#[test]
fn version_probe_accepts_no_newline_and_rejects_stderr() {
    use std::os::unix::fs::PermissionsExt as _;

    // arrange: a receipt-bound executable whose version has no trailing newline.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let active_path = root.join("configs/tui-fidelity-reference-binary-receipt.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(active_path).expect("read active receipt"))
            .expect("parse active receipt");
    let active_revision = value["source"]["revision"]
        .as_str()
        .expect("active source revision")
        .to_owned();
    let temp = tempfile::tempdir().expect("version probe tempdir");
    let binary = temp.path().join("reference");
    let receipt_path = temp.path().join("receipt.json");
    write_probe(&binary, "#!/bin/sh\nprintf fixture-version");
    bind_binary(&mut value, &binary, "fixture-version");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec(&value).expect("serialize receipt"),
    )
    .expect("write receipt");

    // When/Then: an exact stdout version without a newline is accepted.
    ReferenceAuthorityReceipt::read(&receipt_path)
        .and_then(|receipt| receipt.verify(&root, &binary, &active_revision))
        .expect("newline-free version must verify");

    // When/Then: stderr makes the otherwise matching probe fail closed.
    write_probe(
        &binary,
        "#!/bin/sh\nprintf fixture-version\nprintf warning >&2",
    );
    bind_binary(&mut value, &binary, "fixture-version");
    std::fs::write(
        &receipt_path,
        serde_json::to_vec(&value).expect("serialize receipt"),
    )
    .expect("rewrite receipt");
    // act
    let error = ReferenceAuthorityReceipt::read(&receipt_path)
        .and_then(|receipt| receipt.verify(&root, &binary, &active_revision))
        .expect_err("stderr must fail closed");
    // assert
    assert!(error.to_string().contains("stderr"), "{error}");

    fn write_probe(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).expect("write version probe");
        let mut permissions = std::fs::metadata(path)
            .expect("probe metadata")
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions).expect("make probe executable");
    }

    fn bind_binary(value: &mut serde_json::Value, path: &std::path::Path, version: &str) {
        use std::fmt::Write as _;

        let bytes = std::fs::read(path).expect("read version probe");
        let digest =
            Sha256::digest(bytes)
                .iter()
                .fold(String::with_capacity(64), |mut output, byte| {
                    write!(output, "{byte:02x}").expect("write digest");
                    output
                });
        value["binary"]["path"] = serde_json::json!(path);
        value["binary"]["sha256"] = serde_json::json!(digest);
        value["binary"]["version"] = serde_json::json!(version);
    }
}
