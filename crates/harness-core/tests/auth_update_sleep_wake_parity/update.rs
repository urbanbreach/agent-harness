use harness_core::binary_update::{
    apply_update, check_for_update_from_manifest, download_update_artifact, restart_after_update,
    BinaryUpdateApply, BinaryUpdateDownload, LocalUpdateManifest,
};
use harness_core::UnwrapOrAbort;
use sha2::{Digest, Sha256};

#[test]
fn verified_update_pipeline_rejects_bad_hash_and_rolls_back_interruption() {
    // Given: a local update with a known artifact checksum and an existing binary.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let source = temp.path().join("artifact");
    let target = temp.path().join("harness");
    std::fs::write(&source, b"new binary").unwrap_or_abort();
    std::fs::write(&target, b"old binary").unwrap_or_abort();
    let checksum = Sha256::digest(b"new binary")
        .iter()
        .fold(String::new(), |mut acc, byte| {
            use std::fmt::Write;
            let _ = write!(acc, "{byte:02x}");
            acc
        });

    // When: update availability, verified download, apply, mismatch, and interruption paths run.
    let check = check_for_update_from_manifest(
        "1.0.0",
        &LocalUpdateManifest {
            version: "1.1.0".to_string(),
            channel: Some("stable".to_string()),
            min_version: None,
            download_url: None,
            sha256: None,
        },
        None,
    );
    let downloaded = download_update_artifact(
        &format!("file://{}", source.display()),
        Some(&checksum),
        &temp.path().join("downloads"),
    );
    let artifact = match downloaded {
        BinaryUpdateDownload::Downloaded { artifact_path, .. } => artifact_path,
        other => panic!("expected verified download: {other:?}"),
    };
    let applied = apply_update(std::path::Path::new(&artifact), &target);
    let mismatch = download_update_artifact(
        &format!("file://{}", source.display()),
        Some("00"),
        &temp.path().join("mismatch"),
    );
    let interrupted = temp.path().join("interrupted");
    std::fs::write(&interrupted, b"prior binary").unwrap_or_abort();
    let rollback = apply_update(&interrupted, &interrupted);

    // Then: verified update applies, a bad hash is removed, rollback restores the prior binary, and restart is explicit.
    assert!(check.is_update_available() && applied.is_applied() && mismatch.is_unavailable());
    assert_eq!(std::fs::read(&target).unwrap_or_abort(), b"new binary");
    assert!(matches!(
        rollback,
        BinaryUpdateApply::Failed {
            rolled_back: true,
            ..
        }
    ));
    assert_eq!(
        std::fs::read(&interrupted).unwrap_or_abort(),
        b"prior binary"
    );
    assert!(restart_after_update(&temp.path().join("missing"), Some("1.1.0")).restart_needed);
}
