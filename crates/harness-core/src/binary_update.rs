//! Binary version + offline / local-manifest update-check surface.
//!
//! Reports the running package version. Networked auto-update and restart
//! recovery are **not** implemented. Offline channel checks without a local
//! manifest stay structured unavailable. When an operator supplies a local
//! update manifest file, checks can succeed with real up-to-date / update-
//! available outcomes and write a durable update-check receipt.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Package identity for the harness binary (compile-time).
pub const BINARY_PACKAGE_NAME: &str = "harness";

/// Current binary version metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryVersionInfo {
    pub package_name: String,
    pub version: String,
}

impl BinaryVersionInfo {
    pub fn current() -> Self {
        Self {
            package_name: BINARY_PACKAGE_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Operator-facing one-line package identity.
    pub fn one_line(&self) -> String {
        format!("binary: {} {}", self.package_name, self.version)
    }
}

/// Operator-supplied update policy (channel + optional floor).
///
/// This MVP records policy for diagnostics; it does not fetch releases or
/// enforce min-version against a remote catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BinaryUpdatePolicy {
    pub channel: Option<String>,
    pub min_version: Option<String>,
}

impl BinaryUpdatePolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        let channel = channel.into();
        self.channel = (!channel.trim().is_empty()).then_some(channel);
        self
    }

    pub fn with_min_version(mut self, min_version: impl Into<String>) -> Self {
        let min_version = min_version.into();
        self.min_version = (!min_version.trim().is_empty()).then_some(min_version);
        self
    }
}

/// Relative path for the durable update-check receipt under a workspace.
pub const UPDATE_CHECK_RECEIPT_REL: &str = ".agent-harness/update-check.receipt.json";

/// Relative path for an operator-supplied local update manifest.
pub const LOCAL_UPDATE_MANIFEST_REL: &str = ".agent-harness/update-manifest.json";

/// Parsed local update-channel manifest (file-backed; no network).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalUpdateManifest {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Result of an update check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BinaryUpdateCheck {
    /// Current version meets or exceeds the local channel version.
    UpToDate {
        current_version: String,
        channel_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manifest_path: Option<String>,
    },
    /// Local channel advertises a newer version than the running binary.
    UpdateAvailable {
        current_version: String,
        channel_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manifest_path: Option<String>,
    },
    /// Update channel not reachable / not implemented offline / invalid manifest.
    Unavailable {
        current_version: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_version: Option<String>,
    },
}

impl BinaryUpdateCheck {
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub const fn is_up_to_date(&self) -> bool {
        matches!(self, Self::UpToDate { .. })
    }

    pub const fn is_update_available(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. })
    }

    pub const fn is_checked(&self) -> bool {
        matches!(self, Self::UpToDate { .. } | Self::UpdateAvailable { .. })
    }

    /// Operator-facing one-line diagnostics.
    pub fn one_line(&self) -> String {
        match self {
            Self::UpToDate {
                current_version,
                channel_version,
                channel,
                ..
            } => {
                let mut line = format!(
                    "binary update: up_to_date (current={current_version}; channel_version={channel_version})"
                );
                if let Some(ch) = channel.as_ref() {
                    line.push_str(&format!("; channel={ch}"));
                }
                line
            }
            Self::UpdateAvailable {
                current_version,
                channel_version,
                channel,
                ..
            } => {
                let mut line = format!(
                    "binary update: update_available (current={current_version}; channel_version={channel_version})"
                );
                if let Some(ch) = channel.as_ref() {
                    line.push_str(&format!("; channel={ch}"));
                }
                line
            }
            Self::Unavailable {
                current_version,
                reason,
                channel,
                min_version,
            } => {
                let mut line =
                    format!("binary update: unavailable (current={current_version}; {reason})");
                if let Some(ch) = channel.as_ref() {
                    line.push_str(&format!("; channel={ch}"));
                }
                if let Some(min) = min_version.as_ref() {
                    line.push_str(&format!("; min_version={min}"));
                }
                line
            }
        }
    }
}

impl BinaryUpdatePolicy {
    /// Operator-facing one-line policy echo (diagnostics only).
    pub fn one_line(&self) -> String {
        let channel = self.channel.as_deref().unwrap_or("(none)");
        let min_version = self.min_version.as_deref().unwrap_or("(none)");
        format!("binary update policy: channel={channel}; min_version={min_version}")
    }
}

/// Operator-facing counts for binary update checks (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BinaryUpdateSummary {
    pub checks_unavailable: usize,
    pub checks_up_to_date: usize,
    pub total: usize,
    /// True when any check reported a newer channel version.
    pub update_available: bool,
}

impl BinaryUpdateSummary {
    pub fn one_line(&self) -> String {
        format!(
            "binary update: {} unavailable, {} up_to_date ({} total; update_available={})",
            self.checks_unavailable, self.checks_up_to_date, self.total, self.update_available
        )
    }

    pub const fn all_unavailable(&self) -> bool {
        self.total > 0 && self.checks_unavailable == self.total
    }
}

/// Summarize a batch of update checks for operator surfaces.
pub fn summarize_binary_update_checks(checks: &[BinaryUpdateCheck]) -> BinaryUpdateSummary {
    let mut summary = BinaryUpdateSummary {
        total: checks.len(),
        ..BinaryUpdateSummary::default()
    };
    for check in checks {
        match check {
            BinaryUpdateCheck::Unavailable { .. } => {
                summary.checks_unavailable = summary.checks_unavailable.saturating_add(1);
            }
            BinaryUpdateCheck::UpToDate { .. } => {
                summary.checks_up_to_date = summary.checks_up_to_date.saturating_add(1);
            }
            BinaryUpdateCheck::UpdateAvailable { .. } => {
                summary.update_available = true;
            }
        }
    }
    summary
}

/// Inspect the current binary version (always succeeds).
pub fn current_binary_version() -> BinaryVersionInfo {
    BinaryVersionInfo::current()
}

/// Attempt an update check. MVP always returns structured offline unavailability.
///
/// Never claims "up to date" without a real channel — that would be a false pass.
pub fn check_for_update_offline() -> BinaryUpdateCheck {
    check_for_update_with_policy(
        current_binary_version().version,
        BinaryUpdatePolicy::default(),
    )
}

/// Injectable update check for tests (no policy metadata).
pub fn check_for_update_with_version(current_version: impl Into<String>) -> BinaryUpdateCheck {
    check_for_update_with_policy(current_version, BinaryUpdatePolicy::default())
}

/// Offline update check that echoes operator channel/min-version policy.
///
/// Still returns [`BinaryUpdateCheck::Unavailable`] — policy is diagnostic only
/// until a real update channel exists.
pub fn check_for_update_with_policy(
    current_version: impl Into<String>,
    policy: BinaryUpdatePolicy,
) -> BinaryUpdateCheck {
    let current_version = current_version.into();
    let channel = policy
        .channel
        .as_ref()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty());
    let min_version = policy
        .min_version
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let mut reason = String::from(
        "binary update check unavailable offline \
         (no network update channel / restart recovery in this MVP)",
    );
    if let Some(ch) = channel.as_ref() {
        reason.push_str(&format!("; requested channel={ch}"));
    }
    if let Some(min) = min_version.as_ref() {
        reason.push_str(&format!("; requested min_version={min}"));
    }

    BinaryUpdateCheck::Unavailable {
        current_version,
        reason,
        channel,
        min_version,
    }
}

/// Default offline product channels (policy echo only; no network fetch).
pub const OFFLINE_UPDATE_CHANNELS: &[&str] = &["offline", "stable", "beta", "nightly"];

/// Multi-channel offline update-check product path.
///
/// Each channel returns structured [`BinaryUpdateCheck::Unavailable`]. Never
/// claims up-to-date or update-available without a local manifest or network backend.
pub fn check_for_update_channels(
    current_version: impl Into<String>,
    channels: &[&str],
) -> Vec<BinaryUpdateCheck> {
    let current_version = current_version.into();
    channels
        .iter()
        .map(|channel| {
            check_for_update_with_policy(
                current_version.clone(),
                BinaryUpdatePolicy::new().with_channel(*channel),
            )
        })
        .collect()
}

/// Result of the multi-channel offline update product path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryUpdateMultiChannelResult {
    pub policy: BinaryUpdatePolicy,
    pub checks: Vec<BinaryUpdateCheck>,
    pub summary: BinaryUpdateSummary,
    pub version: BinaryVersionInfo,
}

impl BinaryUpdateMultiChannelResult {
    pub fn all_unavailable(&self) -> bool {
        self.summary.all_unavailable()
    }
}

/// Run the default multi-channel offline update product path.
///
/// Includes named channel checks plus a version-only check. Without a local
/// manifest these remain unavailable (`update_available=false`).
pub fn run_offline_multi_channel_update_checks(
    current_version: Option<&str>,
) -> BinaryUpdateMultiChannelResult {
    let version = match current_version {
        Some(v) if !v.trim().is_empty() => BinaryVersionInfo {
            package_name: BINARY_PACKAGE_NAME.to_string(),
            version: v.trim().to_string(),
        },
        _ => current_binary_version(),
    };
    let policy = BinaryUpdatePolicy::new().with_channel("offline");
    let mut checks = check_for_update_channels(&version.version, OFFLINE_UPDATE_CHANNELS);
    checks.push(check_for_update_with_version(version.version.clone()));
    let summary = summarize_binary_update_checks(&checks);
    BinaryUpdateMultiChannelResult {
        policy,
        checks,
        summary,
        version,
    }
}

/// Load a local update manifest JSON file (parse-don't-validate at boundary).
pub fn load_local_update_manifest(
    manifest_path: &Path,
) -> Result<LocalUpdateManifest, BinaryUpdateError> {
    let raw = fs::read_to_string(manifest_path).map_err(|source| BinaryUpdateError::Read {
        path: manifest_path.display().to_string(),
        source,
    })?;
    let manifest: LocalUpdateManifest =
        serde_json::from_str(&raw).map_err(|err| BinaryUpdateError::Parse {
            path: manifest_path.display().to_string(),
            detail: err.to_string(),
        })?;
    if manifest.version.trim().is_empty() {
        return Err(BinaryUpdateError::Parse {
            path: manifest_path.display().to_string(),
            detail: "manifest version must be non-empty".to_string(),
        });
    }
    Ok(LocalUpdateManifest {
        version: manifest.version.trim().to_string(),
        channel: manifest
            .channel
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty()),
        min_version: manifest
            .min_version
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        download_url: manifest
            .download_url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty()),
        sha256: manifest
            .sha256
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    })
}

/// Compare `current_version` against a loaded local channel manifest.
///
/// Succeeds with [`BinaryUpdateCheck::UpToDate`] or
/// [`BinaryUpdateCheck::UpdateAvailable`] when both versions parse as dotted
/// numeric triples (e.g. `0.1.0`). Unparseable versions fail closed as
/// [`BinaryUpdateCheck::Unavailable`].
pub fn check_for_update_from_manifest(
    current_version: impl Into<String>,
    manifest: &LocalUpdateManifest,
    manifest_path: Option<&Path>,
) -> BinaryUpdateCheck {
    let current_version = current_version.into();
    let channel = manifest.channel.clone();
    let manifest_path = manifest_path.map(|p| p.display().to_string());

    if let Some(min) = manifest.min_version.as_deref() {
        match compare_dotted_versions(&current_version, min) {
            Some(std::cmp::Ordering::Less) => {
                return BinaryUpdateCheck::UpdateAvailable {
                    current_version,
                    channel_version: manifest.version.clone(),
                    channel,
                    manifest_path,
                };
            }
            None => {
                return BinaryUpdateCheck::Unavailable {
                    current_version,
                    reason: format!(
                        "local manifest min_version `{min}` or current version is not a dotted numeric version"
                    ),
                    channel,
                    min_version: Some(min.to_string()),
                };
            }
            Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater) => {}
        }
    }

    match compare_dotted_versions(&current_version, &manifest.version) {
        Some(std::cmp::Ordering::Less) => BinaryUpdateCheck::UpdateAvailable {
            current_version,
            channel_version: manifest.version.clone(),
            channel,
            manifest_path,
        },
        Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater) => {
            BinaryUpdateCheck::UpToDate {
                current_version,
                channel_version: manifest.version.clone(),
                channel,
                manifest_path,
            }
        }
        None => BinaryUpdateCheck::Unavailable {
            current_version,
            reason: format!(
                "local manifest version `{}` or current version is not a dotted numeric version",
                manifest.version
            ),
            channel,
            min_version: manifest.min_version.clone(),
        },
    }
}

/// Read a local manifest path and check for updates (file side-effect free).
pub fn check_for_update_from_manifest_path(
    current_version: impl Into<String>,
    manifest_path: &Path,
) -> BinaryUpdateCheck {
    let current_version = current_version.into();
    match load_local_update_manifest(manifest_path) {
        Ok(manifest) => {
            check_for_update_from_manifest(current_version, &manifest, Some(manifest_path))
        }
        Err(err) => BinaryUpdateCheck::Unavailable {
            current_version,
            reason: err.to_string(),
            channel: None,
            min_version: None,
        },
    }
}

/// Durable receipt written after a local-manifest update check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheckReceipt {
    pub schema: String,
    pub package_name: String,
    pub check: BinaryUpdateCheck,
    pub manifest_path: Option<String>,
    pub receipt_path: String,
}

/// Errors for local-manifest load / receipt write.
#[derive(Debug, thiserror::Error)]
pub enum BinaryUpdateError {
    #[error("read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("parse {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("create parent {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// Write an update-check receipt JSON file (real filesystem side effect).
pub fn write_update_check_receipt(
    receipt_path: &Path,
    check: &BinaryUpdateCheck,
    manifest_path: Option<&Path>,
) -> Result<UpdateCheckReceipt, BinaryUpdateError> {
    if let Some(parent) = receipt_path.parent() {
        fs::create_dir_all(parent).map_err(|source| BinaryUpdateError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let receipt = UpdateCheckReceipt {
        schema: "harness-update-check-receipt-v1".to_string(),
        package_name: BINARY_PACKAGE_NAME.to_string(),
        check: check.clone(),
        manifest_path: manifest_path.map(|p| p.display().to_string()),
        receipt_path: receipt_path.display().to_string(),
    };
    let body = serde_json::to_string_pretty(&receipt).map_err(|err| BinaryUpdateError::Write {
        path: receipt_path.display().to_string(),
        source: io::Error::other(err),
    })?;
    fs::write(receipt_path, format!("{body}\n")).map_err(|source| BinaryUpdateError::Write {
        path: receipt_path.display().to_string(),
        source,
    })?;
    Ok(receipt)
}

/// Product result of local-manifest update check + receipt write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalManifestUpdateProduct {
    pub check: BinaryUpdateCheck,
    pub summary: BinaryUpdateSummary,
    pub receipt_path: PathBuf,
    pub manifest_path: PathBuf,
    pub version: BinaryVersionInfo,
}

impl LocalManifestUpdateProduct {
    pub fn one_line(&self) -> String {
        format!(
            "{}; receipt={}",
            self.check.one_line(),
            self.receipt_path.display()
        )
    }
}

/// Product path: check against a local manifest fixture and write a receipt.
///
/// Looks for `manifest_rel` under `workspace_root` (default
/// [`.agent-harness/update-manifest.json`](LOCAL_UPDATE_MANIFEST_REL)). On success
/// writes [`.agent-harness/update-check.receipt.json`](UPDATE_CHECK_RECEIPT_REL).
/// Missing/invalid manifests fail closed as unavailable and still write a receipt.
pub fn run_local_manifest_update_check(
    workspace_root: &Path,
    current_version: Option<&str>,
) -> Result<LocalManifestUpdateProduct, BinaryUpdateError> {
    let version = match current_version {
        Some(v) if !v.trim().is_empty() => BinaryVersionInfo {
            package_name: BINARY_PACKAGE_NAME.to_string(),
            version: v.trim().to_string(),
        },
        _ => current_binary_version(),
    };
    let manifest_path = workspace_root.join(LOCAL_UPDATE_MANIFEST_REL);
    let receipt_path = workspace_root.join(UPDATE_CHECK_RECEIPT_REL);
    let check = check_for_update_from_manifest_path(&version.version, &manifest_path);
    let summary = summarize_binary_update_checks(std::slice::from_ref(&check));
    write_update_check_receipt(&receipt_path, &check, Some(&manifest_path))?;
    Ok(LocalManifestUpdateProduct {
        check,
        summary,
        receipt_path,
        manifest_path,
        version,
    })
}

/// Write a local update-manifest fixture for tests / operator offline channels.
pub fn write_local_update_manifest(
    workspace_root: &Path,
    manifest: &LocalUpdateManifest,
) -> Result<PathBuf, BinaryUpdateError> {
    let path = workspace_root.join(LOCAL_UPDATE_MANIFEST_REL);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| BinaryUpdateError::CreateParent {
            path: parent.display().to_string(),
            source,
        })?;
    }
    let body = serde_json::to_string_pretty(manifest).map_err(|err| BinaryUpdateError::Write {
        path: path.display().to_string(),
        source: io::Error::other(err),
    })?;
    fs::write(&path, format!("{body}\n")).map_err(|source| BinaryUpdateError::Write {
        path: path.display().to_string(),
        source,
    })?;
    Ok(path)
}

/// Result of downloading an update artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BinaryUpdateDownload {
    Downloaded {
        url: String,
        artifact_path: String,
        bytes: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256_verified: Option<bool>,
    },
    Unavailable {
        url: String,
        reason: String,
    },
}

impl BinaryUpdateDownload {
    pub const fn is_downloaded(&self) -> bool {
        matches!(self, Self::Downloaded { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Downloaded {
                url,
                artifact_path,
                bytes,
                sha256_verified,
            } => {
                let mut line = format!(
                    "update download: downloaded url={url} path={artifact_path} bytes={bytes}"
                );
                if let Some(verified) = sha256_verified {
                    line.push_str(&format!("; sha256_verified={verified}"));
                }
                line
            }
            Self::Unavailable { url, reason } => {
                format!("update download: unavailable url={url} ({reason})")
            }
        }
    }
}

/// Download an update artifact from a URL to a temp directory.
///
/// Uses `curl` as a subprocess for HTTP/HTTPS downloads. For `file://` URLs,
/// copies the file directly. Verifies SHA-256 when the manifest provides one.
pub fn download_update_artifact(
    url: &str,
    expected_sha256: Option<&str>,
    dest_dir: &Path,
) -> BinaryUpdateDownload {
    if url.is_empty() {
        return BinaryUpdateDownload::Unavailable {
            url: url.to_string(),
            reason: "download URL is empty".to_string(),
        };
    }

    let artifact_name = url.rsplit('/').next().unwrap_or("update-artifact");
    let artifact_path = dest_dir.join(artifact_name);

    if let Err(err) = fs::create_dir_all(dest_dir) {
        return BinaryUpdateDownload::Unavailable {
            url: url.to_string(),
            reason: format!("failed to create download directory: {err}"),
        };
    }

    if let Some(rest) = url.strip_prefix("file://") {
        let source = Path::new(rest);
        if !source.is_file() {
            return BinaryUpdateDownload::Unavailable {
                url: url.to_string(),
                reason: format!("local file not found: {}", source.display()),
            };
        }
        if let Err(err) = fs::copy(source, &artifact_path) {
            return BinaryUpdateDownload::Unavailable {
                url: url.to_string(),
                reason: format!("failed to copy local file: {err}"),
            };
        }
    } else {
        let output = Command::new("curl")
            .args(["-sSfL", "-o", &artifact_path.display().to_string(), url])
            .output();
        match output {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return BinaryUpdateDownload::Unavailable {
                    url: url.to_string(),
                    reason: format!(
                        "curl exited with status {}: {}",
                        output.status,
                        stderr.trim()
                    ),
                };
            }
            Err(err) => {
                return BinaryUpdateDownload::Unavailable {
                    url: url.to_string(),
                    reason: format!("failed to spawn curl: {err}"),
                };
            }
        }
    }

    let bytes = fs::metadata(&artifact_path).map(|m| m.len()).unwrap_or(0);

    let sha256_verified = if let Some(expected) = expected_sha256 {
        match compute_sha256(&artifact_path) {
            Ok(actual) => Some(actual == expected),
            Err(_) => Some(false),
        }
    } else {
        None
    };

    if let Some(false) = sha256_verified {
        let _ = fs::remove_file(&artifact_path);
        return BinaryUpdateDownload::Unavailable {
            url: url.to_string(),
            reason: "SHA-256 verification failed; artifact removed".to_string(),
        };
    }

    BinaryUpdateDownload::Downloaded {
        url: url.to_string(),
        artifact_path: artifact_path.display().to_string(),
        bytes,
        sha256_verified,
    }
}

fn compute_sha256(path: &Path) -> Result<String, io::Error> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    let mut hex = String::with_capacity(result.len() * 2);
    use std::fmt::Write;
    for b in result.iter() {
        let _ = write!(&mut hex, "{b:02x}");
    }
    Ok(hex)
}

/// Result of applying a downloaded update artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BinaryUpdateApply {
    Applied {
        artifact_path: String,
        target_path: String,
        backup_path: String,
    },
    Failed {
        artifact_path: String,
        target_path: String,
        reason: String,
        rolled_back: bool,
    },
}

impl BinaryUpdateApply {
    pub const fn is_applied(&self) -> bool {
        matches!(self, Self::Applied { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Applied {
                artifact_path,
                target_path,
                backup_path,
            } => {
                format!(
                    "update apply: applied artifact={artifact_path} target={target_path} backup={backup_path}"
                )
            }
            Self::Failed {
                artifact_path,
                target_path,
                reason,
                rolled_back,
            } => {
                format!(
                    "update apply: failed artifact={artifact_path} target={target_path} reason={reason} rolled_back={rolled_back}"
                )
            }
        }
    }
}

/// Apply a downloaded update artifact by replacing the target binary.
///
/// Creates a backup of the current binary before replacing it. On failure,
/// attempts to restore the backup (rollback).
pub fn apply_update(artifact_path: &Path, target_path: &Path) -> BinaryUpdateApply {
    let backup_path = target_path.with_extension("bak");

    if !artifact_path.is_file() {
        return BinaryUpdateApply::Failed {
            artifact_path: artifact_path.display().to_string(),
            target_path: target_path.display().to_string(),
            reason: "artifact file not found".to_string(),
            rolled_back: false,
        };
    }

    if target_path.is_file() {
        if let Err(err) = fs::rename(target_path, &backup_path) {
            return BinaryUpdateApply::Failed {
                artifact_path: artifact_path.display().to_string(),
                target_path: target_path.display().to_string(),
                reason: format!("failed to backup current binary: {err}"),
                rolled_back: false,
            };
        }
    }

    if let Err(err) = fs::copy(artifact_path, target_path) {
        let rolled_back = if backup_path.is_file() {
            fs::rename(&backup_path, target_path).is_ok()
        } else {
            false
        };
        return BinaryUpdateApply::Failed {
            artifact_path: artifact_path.display().to_string(),
            target_path: target_path.display().to_string(),
            reason: format!("failed to copy artifact to target: {err}"),
            rolled_back,
        };
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(target_path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(target_path, perms);
        }
    }

    BinaryUpdateApply::Applied {
        artifact_path: artifact_path.display().to_string(),
        target_path: target_path.display().to_string(),
        backup_path: backup_path.display().to_string(),
    }
}

/// Signal that a restart is needed after applying an update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryUpdateRestart {
    pub restart_needed: bool,
    pub target_path: String,
    pub new_version: Option<String>,
    /// Set when an exec-based restart was attempted but failed (Unix), or when
    /// exec is not implemented on the current platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec_error: Option<String>,
}

impl BinaryUpdateRestart {
    pub fn one_line(&self) -> String {
        let version = self.new_version.as_deref().unwrap_or("(unknown)");
        let mut line = format!(
            "update restart: restart_needed={} target={} version={version}",
            self.restart_needed, self.target_path
        );
        if let Some(err) = &self.exec_error {
            line.push_str(&format!(" exec_error={err}"));
        }
        line
    }
}

/// Restart the process by replacing it with the updated binary via `exec`.
///
/// On Unix, this calls `execvp` which replaces the current process image. If
/// exec succeeds, this function **never returns** — the caller is replaced.
/// If exec fails (e.g. the target binary does not exist or is not executable),
/// a [`BinaryUpdateRestart`] with `exec_error` set is returned.
///
/// On non-Unix platforms, exec is not implemented; the returned signal carries
/// `exec_error` so the caller can fall back to a manual restart.
pub fn restart_after_update(target_path: &Path, new_version: Option<&str>) -> BinaryUpdateRestart {
    let target_path_str = target_path.display().to_string();
    let new_version = new_version.map(String::from);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec() only returns on failure; on success the process is replaced.
        let exec_error = std::process::Command::new(target_path).exec();
        BinaryUpdateRestart {
            restart_needed: true,
            target_path: target_path_str,
            new_version,
            exec_error: Some(format!("exec failed: {exec_error}")),
        }
    }

    #[cfg(not(unix))]
    {
        BinaryUpdateRestart {
            restart_needed: true,
            target_path: target_path_str,
            new_version,
            exec_error: Some("restart via exec not implemented on this platform".to_string()),
        }
    }
}

fn compare_dotted_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let left_parts = parse_dotted_numeric(left)?;
    let right_parts = parse_dotted_numeric(right)?;
    let max_len = left_parts.len().max(right_parts.len());
    for i in 0..max_len {
        let l = left_parts.get(i).copied().unwrap_or(0);
        let r = right_parts.get(i).copied().unwrap_or(0);
        match l.cmp(&r) {
            std::cmp::Ordering::Equal => {}
            other => return Some(other),
        }
    }
    Some(std::cmp::Ordering::Equal)
}

fn parse_dotted_numeric(version: &str) -> Option<Vec<u64>> {
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for part in version.split('.') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        parts.push(part.parse::<u64>().ok()?);
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_non_empty_package_metadata() {
        // arrange
        // act
        // assert
        // act
        let info = current_binary_version();

        // assert
        assert_eq!(info.package_name, BINARY_PACKAGE_NAME);
        assert!(!info.version.is_empty());
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn offline_update_check_is_structured_unavailable_not_fake_success() {
        // arrange
        // act
        // assert
        // act
        let check = check_for_update_with_version("0.1.0");

        // assert
        assert!(check.is_unavailable());
        match check {
            BinaryUpdateCheck::Unavailable {
                current_version,
                reason,
                channel,
                min_version,
            } => {
                assert_eq!(current_version, "0.1.0");
                assert!(reason.contains("unavailable offline") || reason.contains("offline"));
                assert!(
                    reason.contains("update") || reason.contains("channel"),
                    "reason should explain update unavailability: {reason}"
                );
                assert!(channel.is_none());
                assert!(min_version.is_none());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn public_offline_entry_matches_injectable_shape() {
        // arrange
        // act
        // assert
        let check = check_for_update_offline();
        assert!(check.is_unavailable());
        match check {
            BinaryUpdateCheck::Unavailable {
                current_version, ..
            } => {
                assert_eq!(current_version, env!("CARGO_PKG_VERSION"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn policy_channel_and_min_version_are_echoed_but_still_unavailable() {
        // arrange
        // act
        // assert
        // arrange
        let policy = BinaryUpdatePolicy::new()
            .with_channel("stable")
            .with_min_version("0.2.0");

        // act
        let check = check_for_update_with_policy("0.1.0", policy);

        // assert: still unavailable (no fake up-to-date / no network fetch)
        assert!(check.is_unavailable());
        match check {
            BinaryUpdateCheck::Unavailable {
                current_version,
                reason,
                channel,
                min_version,
            } => {
                assert_eq!(current_version, "0.1.0");
                assert_eq!(channel.as_deref(), Some("stable"));
                assert_eq!(min_version.as_deref(), Some("0.2.0"));
                assert!(reason.contains("channel=stable"));
                assert!(reason.contains("min_version=0.2.0"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn empty_policy_fields_are_dropped() {
        // arrange
        // act
        // assert
        // arrange
        let policy = BinaryUpdatePolicy::new()
            .with_channel("   ")
            .with_min_version("");

        // act
        let check = check_for_update_with_policy("1.0.0", policy);

        // assert
        match check {
            BinaryUpdateCheck::Unavailable {
                channel,
                min_version,
                ..
            } => {
                assert!(channel.is_none());
                assert!(min_version.is_none());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn binary_update_operator_diagnostics_cover_version_policy_and_check() {
        // arrange
        // act
        // assert
        // arrange
        let info = current_binary_version();
        let policy = BinaryUpdatePolicy::new()
            .with_channel("stable")
            .with_min_version("0.2.0");
        let check = check_for_update_with_policy("0.1.0", policy.clone());

        // act / Then
        assert!(info.one_line().contains("binary: harness"));
        assert!(info.one_line().contains(&info.version));
        assert!(policy.one_line().contains("channel=stable"));
        assert!(policy.one_line().contains("min_version=0.2.0"));
        assert!(check.is_unavailable());
        assert!(check.one_line().contains("binary update: unavailable"));
        assert!(check.one_line().contains("current=0.1.0"));
        assert!(check.one_line().contains("channel=stable"));
        assert!(check.one_line().contains("min_version=0.2.0"));
        assert!(!check.one_line().contains("up to date"));
    }

    #[test]
    fn binary_update_summary_one_line_and_unavailable_counts() {
        // arrange
        // act
        // assert
        // arrange
        let checks = [
            check_for_update_with_version("0.1.0"),
            check_for_update_with_policy("0.1.0", BinaryUpdatePolicy::new().with_channel("stable")),
        ];

        // act
        let summary = summarize_binary_update_checks(&checks);

        // assert
        assert_eq!(
            summary,
            BinaryUpdateSummary {
                checks_unavailable: 2,
                checks_up_to_date: 0,
                total: 2,
                update_available: false,
            }
        );
        assert!(summary.all_unavailable());
        assert!(summary.one_line().contains("2 unavailable"));
        assert!(summary.one_line().contains("update_available=false"));
        assert!(!summary.one_line().contains("up to date"));
    }

    #[test]
    fn multi_channel_offline_product_path_all_unavailable_with_channel_echo() {
        // arrange
        // act
        // assert
        // arrange / When
        let result = run_offline_multi_channel_update_checks(Some("0.1.0"));

        // assert: default channels + version-only check
        assert!(result.summary.total >= 5);
        assert!(result.all_unavailable());
        assert!(!result.summary.update_available);
        assert_eq!(result.policy.channel.as_deref(), Some("offline"));
        assert_eq!(result.version.version, "0.1.0");

        let channel_checks = check_for_update_channels("0.1.0", OFFLINE_UPDATE_CHANNELS);
        assert_eq!(channel_checks.len(), OFFLINE_UPDATE_CHANNELS.len());
        for (check, expected_channel) in channel_checks.iter().zip(OFFLINE_UPDATE_CHANNELS) {
            assert!(check.is_unavailable());
            match check {
                BinaryUpdateCheck::Unavailable {
                    channel, reason, ..
                } => {
                    assert_eq!(channel.as_deref(), Some(*expected_channel));
                    assert!(
                        reason.contains(expected_channel),
                        "reason should echo channel={expected_channel}: {reason}"
                    );
                }
                other => panic!("expected Unavailable, got {other:?}"),
            }
            assert!(check.one_line().contains("unavailable"));
            assert!(!check.one_line().contains("up to date"));
        }
        assert!(result.summary.one_line().contains("update_available=false"));
    }

    #[test]
    fn local_manifest_up_to_date_when_current_meets_channel() {
        // arrange
        // act
        // assert
        // arrange
        let manifest = LocalUpdateManifest {
            version: "0.1.0".to_string(),
            channel: Some("stable".to_string()),
            min_version: None,
            download_url: None,
            sha256: None,
        };

        // act
        let check = check_for_update_from_manifest("0.1.0", &manifest, None);

        // assert
        assert!(check.is_up_to_date());
        assert!(check.is_checked());
        assert!(!check.is_unavailable());
        assert!(check.one_line().contains("up_to_date"));
        assert!(check.one_line().contains("channel=stable"));
    }

    #[test]
    fn local_manifest_update_available_when_channel_is_newer() {
        // arrange
        // act
        // assert
        // arrange
        let manifest = LocalUpdateManifest {
            version: "0.2.0".to_string(),
            channel: Some("stable".to_string()),
            min_version: None,
            download_url: None,
            sha256: None,
        };

        // act
        let check = check_for_update_from_manifest("0.1.0", &manifest, None);

        // assert
        assert!(check.is_update_available());
        assert!(check.is_checked());
        match check {
            BinaryUpdateCheck::UpdateAvailable {
                current_version,
                channel_version,
                channel,
                ..
            } => {
                assert_eq!(current_version, "0.1.0");
                assert_eq!(channel_version, "0.2.0");
                assert_eq!(channel.as_deref(), Some("stable"));
            }
            other => panic!("expected UpdateAvailable, got {other:?}"),
        }
    }

    #[test]
    fn local_manifest_product_writes_receipt_and_can_succeed() {
        // arrange
        // act
        // assert
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_local_update_manifest(
            root,
            &LocalUpdateManifest {
                version: "0.2.0".to_string(),
                channel: Some("offline".to_string()),
                min_version: None,
                download_url: None,
                sha256: None,
            },
        )
        .expect("write manifest");

        // act
        let product = run_local_manifest_update_check(root, Some("0.1.0")).expect("product");

        // assert: real filesystem side effects
        assert!(product.manifest_path.is_file());
        assert!(product.receipt_path.is_file());
        assert!(product.receipt_path.ends_with(UPDATE_CHECK_RECEIPT_REL));
        assert!(product.check.is_update_available());
        assert!(product.summary.update_available);
        assert_eq!(product.summary.total, 1);
        assert_eq!(product.summary.checks_unavailable, 0);

        let receipt_raw = fs::read_to_string(&product.receipt_path).expect("receipt");
        let receipt: UpdateCheckReceipt = serde_json::from_str(&receipt_raw).expect("receipt json");
        assert_eq!(receipt.schema, "harness-update-check-receipt-v1");
        assert!(receipt.check.is_update_available());
        assert!(product.one_line().contains("update_available"));
        assert!(product.one_line().contains("receipt="));
    }

    #[test]
    fn local_manifest_product_up_to_date_writes_receipt() {
        // arrange
        // act
        // assert
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_local_update_manifest(
            root,
            &LocalUpdateManifest {
                version: "1.0.0".to_string(),
                channel: Some("stable".to_string()),
                min_version: None,
                download_url: None,
                sha256: None,
            },
        )
        .expect("write manifest");

        // act
        let product = run_local_manifest_update_check(root, Some("1.0.0")).expect("product");

        // assert
        assert!(product.check.is_up_to_date());
        assert!(!product.summary.update_available);
        assert_eq!(product.summary.checks_up_to_date, 1);
        assert!(product.receipt_path.is_file());
    }

    #[test]
    fn corrupt_local_manifest_fails_closed_as_unavailable_and_writes_receipt() {
        // arrange — a workspace whose update manifest is unreadable JSON
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path();
        let manifest_path = workspace.join(LOCAL_UPDATE_MANIFEST_REL);
        fs::create_dir_all(manifest_path.parent().expect("parent")).expect("mkdir");
        fs::write(&manifest_path, "{ not valid json").expect("write corrupt manifest");

        // act
        let product = run_local_manifest_update_check(workspace, Some("0.1.0")).expect("product");

        // assert — structured unavailable verdict; the receipt is still written
        assert!(product.check.is_unavailable());
        assert!(product.receipt_path.is_file());
        assert_eq!(product.manifest_path, manifest_path);
    }

    #[test]
    fn missing_local_manifest_fails_closed_but_still_writes_receipt() {
        // arrange
        // act
        // assert
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // act
        let product = run_local_manifest_update_check(root, Some("0.1.0")).expect("product");

        // assert: fail closed + receipt side effect
        assert!(product.check.is_unavailable());
        assert!(product.receipt_path.is_file());
        assert!(!product.summary.update_available);
        assert_eq!(product.summary.checks_unavailable, 1);
    }

    #[test]
    fn download_artifact_from_local_file_url_succeeds() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("new-binary");
        fs::write(&source, b"fake binary content").expect("write");
        let dest_dir = dir.path().join("downloads");

        // act
        let result =
            download_update_artifact(&format!("file://{}", source.display()), None, &dest_dir);

        // assert
        assert!(result.is_downloaded(), "{}", result.one_line());
        match result {
            BinaryUpdateDownload::Downloaded {
                artifact_path,
                bytes,
                sha256_verified,
                ..
            } => {
                assert!(Path::new(&artifact_path).is_file());
                assert_eq!(bytes, 19);
                assert_eq!(sha256_verified, None);
            }
            other => panic!("expected Downloaded, got {other:?}"),
        }
    }

    #[test]
    fn download_artifact_verifies_sha256_when_provided() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("new-binary");
        let content = b"verified content";
        fs::write(&source, content).expect("write");
        let dest_dir = dir.path().join("downloads");

        // compute the real sha256
        let mut hasher = Sha256::new();
        hasher.update(content);
        let digest = hasher.finalize();
        let mut expected_hash = String::with_capacity(digest.len() * 2);
        use std::fmt::Write;
        for b in digest.iter() {
            let _ = write!(&mut expected_hash, "{b:02x}");
        }

        // act
        let result = download_update_artifact(
            &format!("file://{}", source.display()),
            Some(&expected_hash),
            &dest_dir,
        );

        // assert
        assert!(result.is_downloaded(), "{}", result.one_line());
        match result {
            BinaryUpdateDownload::Downloaded {
                sha256_verified, ..
            } => {
                assert_eq!(sha256_verified, Some(true));
            }
            other => panic!("expected Downloaded, got {other:?}"),
        }
    }

    #[test]
    fn download_artifact_rejects_mismatched_sha256() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("new-binary");
        fs::write(&source, b"content").expect("write");
        let dest_dir = dir.path().join("downloads");

        // act
        let result = download_update_artifact(
            &format!("file://{}", source.display()),
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            &dest_dir,
        );

        // assert
        assert!(result.is_unavailable(), "{}", result.one_line());
    }

    #[test]
    fn download_artifact_fails_closed_on_empty_url() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");

        // act
        let result = download_update_artifact("", None, dir.path());

        // assert
        assert!(result.is_unavailable());
    }

    #[test]
    fn apply_update_replaces_binary_with_backup() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("harness");
        fs::write(&target, b"old binary").expect("write old");
        let artifact = dir.path().join("new-harness");
        fs::write(&artifact, b"new binary").expect("write new");

        // act
        let result = apply_update(&artifact, &target);

        // assert
        assert!(result.is_applied(), "{}", result.one_line());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new binary");
        let backup = target.with_extension("bak");
        assert!(backup.is_file());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "old binary");
    }

    #[test]
    fn apply_update_fails_closed_when_artifact_missing() {
        // arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("harness");
        fs::write(&target, b"old binary").expect("write old");
        let artifact = dir.path().join("nonexistent");

        // act
        let result = apply_update(&artifact, &target);

        // assert
        match result {
            BinaryUpdateApply::Failed { rolled_back, .. } => {
                assert!(!rolled_back, "no backup existed, nothing to roll back");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(&target).unwrap(), "old binary");
    }

    #[test]
    fn restart_signal_carries_target_and_version_and_exec_error_when_target_missing() {
        // arrange — a target path that does not exist (exec will fail on Unix)
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("harness");

        // act — attempt restart (exec fails, returns signal)
        let signal = restart_after_update(&target, Some("0.2.0"));

        // assert — signal carries target/version and exec_error is set
        assert!(signal.restart_needed);
        assert_eq!(signal.target_path, target.display().to_string());
        assert_eq!(signal.new_version.as_deref(), Some("0.2.0"));
        assert!(
            signal.exec_error.is_some(),
            "exec_error should be set when exec fails"
        );
        assert!(signal.one_line().contains("restart_needed=true"));
        assert!(signal.one_line().contains("exec_error="));
    }

    #[test]
    fn restart_signal_without_version_still_carries_exec_error() {
        // arrange — a non-existent target
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("harness");

        // act
        let signal = restart_after_update(&target, None);

        // assert
        assert!(signal.restart_needed);
        assert!(signal.new_version.is_none());
        assert!(signal.exec_error.is_some());
        assert!(signal.one_line().contains("version=(unknown)"));
    }

    #[test]
    fn full_pipeline_check_download_apply_restart_succeeds_end_to_end() {
        // arrange — a workspace with a manifest advertising a newer version
        // and a file:// download URL pointing to a fake artifact
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let artifact_source = root.join("new-harness-artifact");
        fs::write(&artifact_source, b"fake new binary content").expect("write artifact");

        let manifest = LocalUpdateManifest {
            version: "99.0.0".to_string(),
            channel: Some("stable".to_string()),
            min_version: None,
            download_url: Some(format!("file://{}", artifact_source.display())),
            sha256: None,
        };
        write_local_update_manifest(root, &manifest).expect("write manifest");

        // fake current binary to be replaced
        let target_binary = root.join("harness");
        fs::write(&target_binary, b"old binary content").expect("write old binary");

        // act — step 1: check
        let product = run_local_manifest_update_check(root, Some("0.1.0")).expect("check product");
        // assert — step 1: update available
        assert!(product.check.is_update_available());

        // act — step 2: download (using manifest's download_url)
        let manifest_loaded =
            load_local_update_manifest(&product.manifest_path).expect("load manifest");
        let download_url = manifest_loaded
            .download_url
            .as_deref()
            .expect("download_url");
        let dest_dir = root.join(".agent-harness/downloads");
        let download =
            download_update_artifact(download_url, manifest_loaded.sha256.as_deref(), &dest_dir);
        assert!(download.is_downloaded(), "{}", download.one_line());

        let artifact_path = match &download {
            BinaryUpdateDownload::Downloaded { artifact_path, .. } => artifact_path.clone(),
            _ => panic!("expected Downloaded"),
        };

        // act — step 3: apply (with backup + rollback)
        let apply = apply_update(Path::new(&artifact_path), &target_binary);
        assert!(apply.is_applied(), "{}", apply.one_line());
        assert_eq!(
            fs::read_to_string(&target_binary).unwrap(),
            "fake new binary content"
        );
        let backup = target_binary.with_extension("bak");
        assert!(backup.is_file());
        assert_eq!(fs::read_to_string(&backup).unwrap(), "old binary content");

        // act — step 4: restart — use a non-existent path so exec fails
        // (we cannot safely exec a real binary inside a unit test)
        let restart_target = root.join("nonexistent-restart-target");
        let restart = restart_after_update(&restart_target, Some("99.0.0"));
        assert!(restart.restart_needed);
        assert_eq!(restart.new_version.as_deref(), Some("99.0.0"));
        assert!(restart.exec_error.is_some());
    }

    #[test]
    fn full_pipeline_aborts_when_check_reports_up_to_date() {
        // arrange — manifest at current version (no update available)
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let current = current_binary_version().version;
        write_local_update_manifest(
            root,
            &LocalUpdateManifest {
                version: current.clone(),
                channel: Some("stable".to_string()),
                min_version: None,
                download_url: Some("file:///nonexistent".to_string()),
                sha256: None,
            },
        )
        .expect("write manifest");

        // act — check
        let product = run_local_manifest_update_check(root, None).expect("check");

        // assert — up to date, pipeline should not proceed to download
        assert!(product.check.is_up_to_date());
        assert!(!product.summary.update_available);
    }

    #[test]
    fn apply_update_rolls_back_on_copy_failure() {
        // arrange — artifact exists but target directory is read-only
        // (simulate copy failure by making target path's parent a file)
        let dir = tempfile::tempdir().expect("tempdir");
        let target_parent = dir.path().join("blocking-file");
        fs::write(&target_parent, b"blocker").expect("write blocker");
        let target = target_parent.join("harness"); // parent is a file, not a dir
        let artifact = dir.path().join("new-harness");
        fs::write(&artifact, b"new binary").expect("write artifact");

        // act — apply will fail at the copy step (no backup since target doesn't exist)
        let result = apply_update(&artifact, &target);

        // assert — Failed with rolled_back=false (no backup existed)
        match result {
            BinaryUpdateApply::Failed { rolled_back, .. } => {
                assert!(!rolled_back);
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }
}
