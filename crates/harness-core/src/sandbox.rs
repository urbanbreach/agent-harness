//! Coordinator-owned OS sandbox *policy* surface (Landlock MVP).
//!
//! Operator **permission** approval (`docs/permissions.md`) is an approval layer,
//! **not** a sandbox. This module models OS/process confinement intent: public
//! policy modes, Landlock detection, FS plans, and whether confinement was
//! actually applied. Unavailable is never a silent allow.
//!
//! Landlock MVP: detect LSM → build FS plan → apply only via explicit hook
//! (bash child `pre_exec` at [`BASH_SPAWN_SANDBOX_INTEGRATION`]). Default prepare
//! fails closed for non-`Off` until that hook is wired. Network / Seatbelt /
//! full profiles remain residual.

mod landlock;
mod prepare;
mod product;

#[cfg(test)]
#[path = "sandbox/sandbox_tests.rs"]
mod sandbox_tests;

pub use landlock::{
    apply_landlock_fs_plan, apply_landlock_fs_plan_not_implemented, build_fs_plan,
    describe_fs_plan_for_policy, detect_landlock, detect_landlock_with, lsm_list_contains_landlock,
    probe_landlock_lsm, summarize_fs_plan, LandlockSupport, SandboxFsPlan, SandboxFsPlanSummary,
    SandboxPathRoots, BASH_SPAWN_SANDBOX_INTEGRATION,
};
pub use prepare::{
    evaluate_availability, evaluate_availability_for_platform, evaluate_availability_with_landlock,
    prepare_sandbox, prepare_sandbox_for_platform, prepare_sandbox_for_spawn,
};
pub use product::{
    probe_os_sandbox_product, probe_os_sandbox_product_for_platform, OsSandboxProductProbe,
    OS_SANDBOX_ENFORCED_POLICIES,
};

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Public sandbox policy modes (product vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPolicy {
    /// No OS sandbox requested.
    Off,
    /// Read broadly; write limited to workspace + temp + harness state.
    WorkspaceWrite,
    /// Read broadly; write only to harness state + temp.
    ReadOnly,
    /// Tightest profile: restricted reads, limited writes.
    Strict,
}

impl SandboxPolicy {
    /// Parse a public policy name (snake_case or kebab-case).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "off" => Some(Self::Off),
            "workspace" | "workspace_write" | "workspace-write" => Some(Self::WorkspaceWrite),
            "read_only" | "read-only" | "readonly" => Some(Self::ReadOnly),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }

    /// Stable public id for config / inventory / diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::WorkspaceWrite => "workspace_write",
            Self::ReadOnly => "read_only",
            Self::Strict => "strict",
        }
    }

    /// True when this policy requests OS-level confinement (not a no-op).
    pub const fn requires_enforcement(self) -> bool {
        !matches!(self, Self::Off)
    }
}

impl fmt::Display for SandboxPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Host platform bucket for availability reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPlatform {
    Linux,
    Macos,
    Windows,
    Other,
}

impl SandboxPlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Other => "other",
        }
    }
}

impl fmt::Display for SandboxPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether OS-level enforcement can be applied for a policy on this host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SandboxAvailability {
    /// Enforcement is available (or not required for [`SandboxPolicy::Off`]).
    Available { platform: SandboxPlatform },
    /// Enforcement cannot be applied; must not be reported as success.
    Unavailable {
        platform: SandboxPlatform,
        reason: String,
    },
}

impl SandboxAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line availability (presence ≠ confinement applied).
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { platform } => {
                format!("sandbox availability: available ({})", platform.as_str())
            }
            Self::Unavailable { platform, reason } => {
                format!(
                    "sandbox availability: unavailable ({}: {})",
                    platform.as_str(),
                    reason
                )
            }
        }
    }
}

/// Result of preparing sandbox enforcement before spawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SandboxPrepareResult {
    /// Policy is [`SandboxPolicy::Off`]; no confinement requested.
    NotRequired { platform: SandboxPlatform },
    /// Confinement was **applied** (requires successful apply hook).
    Prepared {
        policy: SandboxPolicy,
        platform: SandboxPlatform,
    },
    /// Confinement was requested but cannot be applied.
    Unavailable {
        policy: SandboxPolicy,
        platform: SandboxPlatform,
        reason: String,
    },
}

impl SandboxPrepareResult {
    /// True when spawn may proceed without claiming false OS confinement.
    pub const fn allows_spawn_without_false_success(&self) -> bool {
        matches!(self, Self::NotRequired { .. } | Self::Prepared { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line prepare diagnostics (not enforcement proof).
    pub fn one_line(&self) -> String {
        match self {
            Self::NotRequired { platform } => {
                format!("sandbox prepare: not_required ({})", platform.as_str())
            }
            Self::Prepared { policy, platform } => {
                format!(
                    "sandbox prepare: prepared policy={} ({})",
                    policy.as_str(),
                    platform.as_str()
                )
            }
            Self::Unavailable {
                policy,
                platform,
                reason,
            } => {
                format!(
                    "sandbox prepare: unavailable policy={} ({}: {})",
                    policy.as_str(),
                    platform.as_str(),
                    reason
                )
            }
        }
    }
}

/// Sandbox policy / availability errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SandboxError {
    #[error("unknown sandbox policy: {value}")]
    UnknownPolicy { value: String },
}

/// Detect the current host platform bucket.
pub fn current_platform() -> SandboxPlatform {
    if cfg!(target_os = "linux") {
        SandboxPlatform::Linux
    } else if cfg!(target_os = "macos") {
        SandboxPlatform::Macos
    } else if cfg!(target_os = "windows") {
        SandboxPlatform::Windows
    } else {
        SandboxPlatform::Other
    }
}

/// Parse policy or return a typed error (invalid-policy journey).
pub fn require_policy(value: &str) -> Result<SandboxPolicy, SandboxError> {
    SandboxPolicy::parse(value).ok_or_else(|| SandboxError::UnknownPolicy {
        value: value.to_string(),
    })
}

/// One public OS sandbox profile with host availability (not enforcement proof).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsSandboxProfile {
    pub policy: SandboxPolicy,
    pub availability: SandboxAvailability,
}

impl OsSandboxProfile {
    pub const fn policy_id(&self) -> &'static str {
        self.policy.as_str()
    }

    pub const fn is_available(&self) -> bool {
        self.availability.is_available()
    }

    /// Operator-facing one-line profile diagnostics (not enforcement proof).
    pub fn one_line(&self) -> String {
        let avail = if self.is_available() {
            "available"
        } else {
            "unavailable"
        };
        format!(
            "OS sandbox profile: policy={} ({})",
            self.policy_id(),
            avail
        )
    }
}

/// Canonical public policy set for inventory / diagnostics listing.
pub const OS_SANDBOX_POLICIES: &[SandboxPolicy] = &[
    SandboxPolicy::Off,
    SandboxPolicy::WorkspaceWrite,
    SandboxPolicy::ReadOnly,
    SandboxPolicy::Strict,
];

/// List OS sandbox profiles with availability for the current host.
pub fn list_os_profiles() -> Vec<OsSandboxProfile> {
    list_os_profiles_for_platform(current_platform())
}

/// Platform-injectable profile listing (tests + diagnostics).
pub fn list_os_profiles_for_platform(platform: SandboxPlatform) -> Vec<OsSandboxProfile> {
    let landlock = detect_landlock();
    OS_SANDBOX_POLICIES
        .iter()
        .copied()
        .map(|policy| OsSandboxProfile {
            policy,
            availability: evaluate_availability_with_landlock(policy, platform, &landlock),
        })
        .collect()
}

/// Operator-facing counts for OS sandbox profile listing (diagnostics only).
///
/// Presence of profiles does **not** prove child confinement was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OsSandboxProfilesSummary {
    pub total: usize,
    pub available: usize,
    pub unavailable: usize,
}

impl OsSandboxProfilesSummary {
    pub fn one_line(&self) -> String {
        format!(
            "OS sandbox profiles: {} total ({} available, {} unavailable)",
            self.total, self.available, self.unavailable
        )
    }
}

/// Summarize [`list_os_profiles`] results for operator surfaces.
pub fn summarize_os_profiles(profiles: &[OsSandboxProfile]) -> OsSandboxProfilesSummary {
    let mut summary = OsSandboxProfilesSummary {
        total: profiles.len(),
        ..OsSandboxProfilesSummary::default()
    };
    for profile in profiles {
        if profile.is_available() {
            summary.available = summary.available.saturating_add(1);
        } else {
            summary.unavailable = summary.unavailable.saturating_add(1);
        }
    }
    summary
}
