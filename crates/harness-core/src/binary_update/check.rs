use super::*;
use crate::store::{lock_private_parent, read_private_bytes, write_private_atomic};
use semver::Version;

fn version(value: &str) -> Option<Version> {
    if value.len() > 128 {
        return None;
    }
    Version::parse(value.strip_prefix('v').unwrap_or(value)).ok()
}
fn validate(manifest: &LocalUpdateManifest) -> Result<(), &'static str> {
    if version(&manifest.version).is_none() {
        return Err("manifest version is not semantic versioning");
    }
    if manifest
        .min_version
        .as_deref()
        .is_some_and(|s| version(s).is_none())
    {
        return Err("manifest minimum version is invalid");
    }
    if manifest.channel.as_deref().is_some_and(|s| {
        s.is_empty()
            || s.len() > 64
            || !s
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    }) {
        return Err("manifest channel is invalid");
    }
    if manifest.sha256.as_deref().is_some_and(|s| !valid_digest(s)) {
        return Err("manifest SHA-256 is invalid");
    }
    if manifest.download_url.as_deref().is_some_and(|s| {
        s.len() > 8192
            || install::parse_url(s).is_err()
            || crate::redact::DefaultRedactor::default().secret_finding_count(s) > 0
    }) {
        return Err("manifest download URL is invalid");
    }
    Ok(())
}
pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
pub fn check_for_update_offline() -> BinaryUpdateCheck {
    check_for_update_with_version(env!("CARGO_PKG_VERSION"))
}
pub fn check_for_update_with_version(current_version: impl Into<String>) -> BinaryUpdateCheck {
    check_for_update_with_policy(current_version, BinaryUpdatePolicy::default())
}
pub fn check_for_update_with_policy(
    current_version: impl Into<String>,
    policy: BinaryUpdatePolicy,
) -> BinaryUpdateCheck {
    BinaryUpdateCheck::Unavailable {
        current_version: current_version.into(),
        channel: policy.channel,
        min_version: policy.min_version,
        reason: "no local channel manifest supplied; no network check performed".into(),
    }
}
pub fn check_for_update_channels(
    current_version: impl Into<String>,
    channels: &[&str],
) -> Vec<BinaryUpdateCheck> {
    let current = current_version.into();
    channels
        .iter()
        .map(|channel| {
            check_for_update_with_policy(&current, BinaryUpdatePolicy::new().with_channel(*channel))
        })
        .collect()
}
pub fn run_offline_multi_channel_update_checks(
    current: Option<&str>,
) -> BinaryUpdateMultiChannelResult {
    let version = BinaryVersionInfo {
        package_name: BINARY_PACKAGE_NAME.into(),
        version: current.unwrap_or(env!("CARGO_PKG_VERSION")).into(),
    };
    let mut checks = vec![check_for_update_with_version(&version.version)];
    checks.extend(check_for_update_channels(
        &version.version,
        OFFLINE_UPDATE_CHANNELS,
    ));
    BinaryUpdateMultiChannelResult {
        policy: BinaryUpdatePolicy::new().with_channel("offline"),
        summary: summarize_binary_update_checks(&checks),
        version,
        checks,
    }
}
pub fn load_local_update_manifest(path: &Path) -> Result<LocalUpdateManifest, BinaryUpdateError> {
    let bytes = read_private_bytes(path, 64 * 1024)
        .map_err(|source| BinaryUpdateError::Read {
            path: path.display().to_string(),
            source,
        })?
        .ok_or_else(|| BinaryUpdateError::Read {
            path: path.display().to_string(),
            source: io::Error::from(io::ErrorKind::NotFound),
        })?;
    let manifest =
        serde_json::from_slice(&bytes).map_err(|_| invalid(path, "invalid manifest JSON"))?;
    validate(&manifest).map_err(|detail| invalid(path, detail))?;
    Ok(manifest)
}
pub fn check_for_update_from_manifest(
    current_version: impl Into<String>,
    manifest: &LocalUpdateManifest,
    path: Option<&Path>,
) -> BinaryUpdateCheck {
    let current_version = current_version.into();
    let failed = |reason: &str| BinaryUpdateCheck::Unavailable {
        current_version: current_version.clone(),
        channel: manifest.channel.clone(),
        min_version: manifest.min_version.clone(),
        reason: reason.into(),
    };
    if let Err(reason) = validate(manifest) {
        return failed(reason);
    }
    let (Some(current), Some(latest)) = (version(&current_version), version(&manifest.version))
    else {
        return failed("invalid current or channel version");
    };
    if let Some(minimum) = manifest.min_version.as_deref().and_then(version)
        && current.cmp_precedence(&minimum).is_lt()
    {
        return failed("current version is below the minimum supported for this update");
    }
    let channel = manifest.channel.clone();
    let channel_version = manifest.version.clone();
    let manifest_path = path.map(|p| p.display().to_string());
    if latest.cmp_precedence(&current).is_gt() {
        BinaryUpdateCheck::UpdateAvailable {
            current_version,
            channel_version,
            channel,
            manifest_path,
        }
    } else {
        BinaryUpdateCheck::UpToDate {
            current_version,
            channel_version,
            channel,
            manifest_path,
        }
    }
}
pub fn check_for_update_from_manifest_path(
    current_version: impl Into<String>,
    path: &Path,
) -> BinaryUpdateCheck {
    read_manifest_check(current_version.into(), path).0
}
fn read_manifest_check(
    current: String,
    path: &Path,
) -> (BinaryUpdateCheck, Option<LocalUpdateManifest>) {
    let manifest = load_local_update_manifest(path).ok();
    let check = match &manifest {
        Some(manifest) => check_for_update_from_manifest(current, manifest, Some(path)),
        None => BinaryUpdateCheck::Unavailable {
            current_version: current,
            reason: "local update manifest is missing or invalid".into(),
            channel: None,
            min_version: None,
        },
    };
    (check, manifest)
}
pub fn write_update_check_receipt(
    path: &Path,
    check: &BinaryUpdateCheck,
    manifest_path: Option<&Path>,
) -> Result<UpdateCheckReceipt, BinaryUpdateError> {
    let receipt = UpdateCheckReceipt {
        schema: "harness-update-check.v1".into(),
        package_name: BINARY_PACKAGE_NAME.into(),
        check: check.clone(),
        manifest_path: manifest_path.map(|p| p.display().to_string()),
        receipt_path: path.display().to_string(),
    };
    save(path, &receipt)?;
    Ok(receipt)
}
pub fn run_local_manifest_update_check(
    root: &Path,
    current: Option<&str>,
) -> Result<LocalManifestUpdateProduct, BinaryUpdateError> {
    let version = BinaryVersionInfo {
        package_name: BINARY_PACKAGE_NAME.into(),
        version: current.unwrap_or(env!("CARGO_PKG_VERSION")).into(),
    };
    let manifest_path = root.join(LOCAL_UPDATE_MANIFEST_REL);
    let receipt_path = root.join(UPDATE_CHECK_RECEIPT_REL);
    let (check, manifest) = read_manifest_check(version.version.clone(), &manifest_path);
    write_update_check_receipt(&receipt_path, &check, Some(&manifest_path))?;
    Ok(LocalManifestUpdateProduct {
        summary: summarize_binary_update_checks(std::slice::from_ref(&check)),
        check,
        manifest,
        version,
        receipt_path,
        manifest_path,
    })
}
pub fn write_local_update_manifest(
    root: &Path,
    manifest: &LocalUpdateManifest,
) -> Result<PathBuf, BinaryUpdateError> {
    let path = root.join(LOCAL_UPDATE_MANIFEST_REL);
    validate(manifest).map_err(|detail| invalid(&path, detail))?;
    save(&path, manifest)?;
    Ok(path)
}
fn invalid(path: &Path, detail: &str) -> BinaryUpdateError {
    BinaryUpdateError::Parse {
        path: path.display().to_string(),
        detail: detail.into(),
    }
}
fn save(path: &Path, value: &impl Serialize) -> Result<(), BinaryUpdateError> {
    let result = (|| -> io::Result<()> {
        let value = serde_json::to_value(value)?;
        let value = crate::redact::redact_value(&crate::redact::DefaultRedactor::default(), &value);
        let bytes = serde_json::to_vec_pretty(&value)?;
        let _lock = lock_private_parent(path)?;
        write_private_atomic(path, &bytes)
    })();
    result.map_err(|source| BinaryUpdateError::Write {
        path: path.display().to_string(),
        source,
    })
}
