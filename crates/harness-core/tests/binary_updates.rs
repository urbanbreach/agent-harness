use harness_core::binary_update::*;
use sha2::{Digest, Sha256};

#[test]
fn local_update_checks_use_semver_and_never_write_until_a_receipt_is_requested(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join(LOCAL_UPDATE_MANIFEST_REL);
    assert!(check_for_update_from_manifest_path("1.0.0", &path).is_unavailable());
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    for (current, next, available) in [
        ("1.9.0", "1.10.0", true),
        ("1.0.0-rc.1", "1.0.0", true),
        ("1.0.0", "1.0.0-beta.1", false),
        ("1.2.0+one", "1.2.0+two", false),
    ] {
        let manifest = LocalUpdateManifest {
            version: next.into(),
            channel: Some("stable".into()),
            min_version: None,
            download_url: None,
            sha256: None,
        };
        write_local_update_manifest(temp.path(), &manifest)?;
        let check = check_for_update_from_manifest_path(current, &path);
        assert!(check.is_checked());
        assert_eq!(check.is_update_available(), available);
    }
    assert!(!temp.path().join(UPDATE_CHECK_RECEIPT_REL).exists());
    let product = run_local_manifest_update_check(temp.path(), Some("1.0.0"))?;
    assert!(product.check.is_update_available());
    assert!(product.receipt_path.is_file());
    let before = std::fs::read(&path)?;
    let mut manifest = load_local_update_manifest(&path)?;
    assert_eq!(product.manifest.as_ref(), Some(&manifest));
    manifest.download_url =
        Some("https://example.test/update?access_token=private-update-secret".into());
    assert!(write_local_update_manifest(temp.path(), &manifest).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    std::fs::write(&path, r#"{"version":"unknown"}"#)?;
    assert_eq!(
        product.manifest.as_ref().map(|m| m.version.as_str()),
        Some("1.2.0+two")
    );
    assert!(check_for_update_from_manifest_path("1.0.0", &path).is_unavailable());
    assert_eq!(std::fs::read_to_string(path)?, r#"{"version":"unknown"}"#);
    Ok(())
}

#[test]
fn downloaded_updates_verify_before_publication_and_replace_with_a_preserved_backup(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("new binary");
    let target = temp.path().join("installed");
    let downloads = temp.path().join("downloads");
    std::fs::write(&source, b"new executable")?;
    std::fs::write(&target, b"old executable")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
    }
    let url = reqwest::Url::from_file_path(&source).map_err(|()| "invalid file URL")?;
    let mismatch = download_update_artifact(url.as_str(), Some(&"0".repeat(64)), &downloads);
    assert!(mismatch.is_unavailable());
    assert_eq!(std::fs::read_dir(&downloads)?.count(), 0);
    let sha = hex::encode(Sha256::digest(b"new executable"));
    let result = download_update_artifact(url.as_str(), Some(&sha), &downloads);
    let BinaryUpdateDownload::Downloaded {
        artifact_path,
        sha256_verified: Some(true),
        ..
    } = result
    else {
        return Err(result.one_line().into());
    };
    let apply = apply_update(std::path::Path::new(&artifact_path), &target);
    let BinaryUpdateApply::Applied { backup_path, .. } = apply else {
        return Err(apply.one_line().into());
    };
    assert_eq!(std::fs::read(&target)?, b"new executable");
    assert_eq!(std::fs::read(&backup_path)?, b"old executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&target)?.permissions().mode() & 0o777,
            0o755
        );
    }
    assert!(!apply_update(std::path::Path::new(&artifact_path), &target).is_applied());
    assert_eq!(std::fs::read(&backup_path)?, b"old executable");
    assert_eq!(std::fs::read(&target)?, b"new executable");
    let invalid =
        download_update_artifact("https://user:private@example.test/update", None, &downloads);
    assert!(!invalid.one_line().contains("private"));
    Ok(())
}
