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

use serde::{Deserialize, Serialize};

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
        // When
        let info = current_binary_version();

        // Then
        assert_eq!(info.package_name, BINARY_PACKAGE_NAME);
        assert!(!info.version.is_empty());
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn offline_update_check_is_structured_unavailable_not_fake_success() {
        // arrange
        // act
        // assert
        // When
        let check = check_for_update_with_version("0.1.0");

        // Then
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
        // Given
        let policy = BinaryUpdatePolicy::new()
            .with_channel("stable")
            .with_min_version("0.2.0");

        // When
        let check = check_for_update_with_policy("0.1.0", policy);

        // Then: still unavailable (no fake up-to-date / no network fetch)
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
        // Given
        let policy = BinaryUpdatePolicy::new()
            .with_channel("   ")
            .with_min_version("");

        // When
        let check = check_for_update_with_policy("1.0.0", policy);

        // Then
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
        // Given
        let info = current_binary_version();
        let policy = BinaryUpdatePolicy::new()
            .with_channel("stable")
            .with_min_version("0.2.0");
        let check = check_for_update_with_policy("0.1.0", policy.clone());

        // When / Then
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
        // Given
        let checks = [
            check_for_update_with_version("0.1.0"),
            check_for_update_with_policy("0.1.0", BinaryUpdatePolicy::new().with_channel("stable")),
        ];

        // When
        let summary = summarize_binary_update_checks(&checks);

        // Then
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
        // Given / When
        let result = run_offline_multi_channel_update_checks(Some("0.1.0"));

        // Then: default channels + version-only check
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
        // Given
        let manifest = LocalUpdateManifest {
            version: "0.1.0".to_string(),
            channel: Some("stable".to_string()),
            min_version: None,
        };

        // When
        let check = check_for_update_from_manifest("0.1.0", &manifest, None);

        // Then
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
        // Given
        let manifest = LocalUpdateManifest {
            version: "0.2.0".to_string(),
            channel: Some("stable".to_string()),
            min_version: None,
        };

        // When
        let check = check_for_update_from_manifest("0.1.0", &manifest, None);

        // Then
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
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_local_update_manifest(
            root,
            &LocalUpdateManifest {
                version: "0.2.0".to_string(),
                channel: Some("offline".to_string()),
                min_version: None,
            },
        )
        .expect("write manifest");

        // When
        let product = run_local_manifest_update_check(root, Some("0.1.0")).expect("product");

        // Then: real filesystem side effects
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
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_local_update_manifest(
            root,
            &LocalUpdateManifest {
                version: "1.0.0".to_string(),
                channel: Some("stable".to_string()),
                min_version: None,
            },
        )
        .expect("write manifest");

        // When
        let product = run_local_manifest_update_check(root, Some("1.0.0")).expect("product");

        // Then
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
        // Given
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // When
        let product = run_local_manifest_update_check(root, Some("0.1.0")).expect("product");

        // Then: fail closed + receipt side effect
        assert!(product.check.is_unavailable());
        assert!(product.receipt_path.is_file());
        assert!(!product.summary.update_available);
        assert_eq!(product.summary.checks_unavailable, 1);
    }
}
