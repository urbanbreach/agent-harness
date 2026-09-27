//! Offline version checks and explicit download, replacement, and restart operations.
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
mod check;
mod install;
pub use check::*;
pub use install::*;

pub const BINARY_PACKAGE_NAME: &str = "harness";
pub const UPDATE_CHECK_RECEIPT_REL: &str = ".agent-harness/update-check.receipt.json";
pub const LOCAL_UPDATE_MANIFEST_REL: &str = ".agent-harness/update-manifest.json";
pub const OFFLINE_UPDATE_CHANNELS: &[&str] = &["offline", "stable", "beta", "nightly"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryVersionInfo {
    pub package_name: String,
    pub version: String,
}
impl BinaryVersionInfo {
    pub fn current() -> Self {
        Self {
            package_name: BINARY_PACKAGE_NAME.into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    pub fn one_line(&self) -> String {
        format!("{} {}", self.package_name, self.version)
    }
}
pub fn current_binary_version() -> BinaryVersionInfo {
    BinaryVersionInfo::current()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryUpdatePolicy {
    pub channel: Option<String>,
    pub min_version: Option<String>,
}
impl BinaryUpdatePolicy {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.channel = Some(channel.into());
        self
    }
    pub fn with_min_version(mut self, version: impl Into<String>) -> Self {
        self.min_version = Some(version.into());
        self
    }
    pub fn one_line(&self) -> String {
        format!(
            "channel={} min_version={}",
            self.channel.as_deref().unwrap_or("unspecified"),
            self.min_version.as_deref().unwrap_or("unspecified")
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BinaryUpdateCheck {
    UpToDate {
        current_version: String,
        channel_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manifest_path: Option<String>,
    },
    UpdateAvailable {
        current_version: String,
        channel_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        channel: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        manifest_path: Option<String>,
    },
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
        !self.is_unavailable()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::UpToDate { current_version, channel_version, channel, .. } => format!("binary update: up_to_date current={current_version} latest={channel_version} channel={}", channel.as_deref().unwrap_or("local")),
            Self::UpdateAvailable { current_version, channel_version, channel, .. } => format!("binary update: available current={current_version} latest={channel_version} channel={}", channel.as_deref().unwrap_or("local")),
            Self::Unavailable { current_version, reason, channel, .. } => format!("binary update: unavailable current={current_version} channel={}: {reason}", channel.as_deref().unwrap_or("offline")),
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryUpdateSummary {
    pub checks_unavailable: usize,
    pub checks_up_to_date: usize,
    pub total: usize,
    pub update_available: bool,
}
impl BinaryUpdateSummary {
    pub const fn all_unavailable(&self) -> bool {
        self.total > 0 && self.checks_unavailable == self.total
    }
    pub fn one_line(&self) -> String {
        format!(
            "total={} unavailable={} up_to_date={} update_available={}",
            self.total, self.checks_unavailable, self.checks_up_to_date, self.update_available
        )
    }
}
pub fn summarize_binary_update_checks(checks: &[BinaryUpdateCheck]) -> BinaryUpdateSummary {
    BinaryUpdateSummary {
        total: checks.len(),
        checks_unavailable: checks.iter().filter(|c| c.is_unavailable()).count(),
        checks_up_to_date: checks.iter().filter(|c| c.is_up_to_date()).count(),
        update_available: checks.iter().any(BinaryUpdateCheck::is_update_available),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckReceipt {
    pub schema: String,
    pub package_name: String,
    pub check: BinaryUpdateCheck,
    pub manifest_path: Option<String>,
    pub receipt_path: String,
}
#[derive(Debug, Clone)]
pub struct LocalManifestUpdateProduct {
    pub check: BinaryUpdateCheck,
    pub manifest: Option<LocalUpdateManifest>,
    pub summary: BinaryUpdateSummary,
    pub receipt_path: PathBuf,
    pub manifest_path: PathBuf,
    pub version: BinaryVersionInfo,
}
impl LocalManifestUpdateProduct {
    pub fn one_line(&self) -> String {
        self.check.one_line()
    }
}
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
        !self.is_downloaded()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Downloaded { artifact_path, bytes, sha256_verified, .. } => format!("update download: downloaded path={artifact_path} bytes={bytes} sha256_verified={sha256_verified:?}"),
            Self::Unavailable { reason, .. } => format!("update download: unavailable {reason}"),
        }
    }
}
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
            Self::Applied { target_path, .. } => {
                format!("update apply: applied target={target_path}")
            }
            Self::Failed {
                reason,
                rolled_back,
                ..
            } => format!("update apply: failed {reason} rolled_back={rolled_back}"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryUpdateRestart {
    pub restart_needed: bool,
    pub target_path: String,
    pub new_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec_error: Option<String>,
}
impl BinaryUpdateRestart {
    pub fn one_line(&self) -> String {
        format!(
            "update restart: needed={} error={}",
            self.restart_needed,
            self.exec_error.as_deref().unwrap_or("none")
        )
    }
}
pub fn restart_after_update(
    target_path: &Path,
    new_version: Option<&str>,
    workspace: &Path,
) -> BinaryUpdateRestart {
    #[cfg(unix)]
    let error = {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new(target_path);
        // Reusing the update command's arguments would run the update again after exec.
        command.current_dir(workspace);
        let error = command.exec();
        format!("restart failed: {}", error.kind())
    };
    #[cfg(not(unix))]
    let error = {
        let _ = workspace;
        "automatic restart requires Unix; restart manually".to_owned()
    };
    BinaryUpdateRestart {
        restart_needed: true,
        target_path: target_path.display().to_string(),
        new_version: new_version.map(str::to_owned),
        exec_error: Some(error),
    }
}
