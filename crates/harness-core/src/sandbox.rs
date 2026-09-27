//! Explicit child-process confinement. Inspecting a profile never applies it.
use serde::{Deserialize, Serialize};
mod filesystem;
mod network;
mod product;
pub use filesystem::*;
pub use network::*;
pub use product::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPolicy {
    Off,
    WorkspaceWrite,
    ReadOnly,
    Strict,
}
impl SandboxPolicy {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "off" => Self::Off,
            "workspace" | "workspace_write" | "workspace-write" => Self::WorkspaceWrite,
            "readonly" | "read_only" | "read-only" => Self::ReadOnly,
            "strict" => Self::Strict,
            _ => return None,
        })
    }
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::WorkspaceWrite => "workspace_write",
            Self::ReadOnly => "read_only",
            Self::Strict => "strict",
        }
    }
}
impl std::fmt::Display for SandboxPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
impl std::fmt::Display for SandboxPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
pub const fn current_platform() -> SandboxPlatform {
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
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("unknown sandbox policy: {value}")]
    UnknownPolicy { value: String },
}
pub fn require_policy(value: &str) -> Result<SandboxPolicy, SandboxError> {
    SandboxPolicy::parse(value).ok_or_else(|| SandboxError::UnknownPolicy {
        value: value.into(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SandboxAvailability {
    Available {
        platform: SandboxPlatform,
    },
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
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { platform } => format!("available platform={platform}"),
            Self::Unavailable { platform, reason } => {
                format!("unavailable platform={platform}: {reason}")
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum SandboxPrepareResult {
    NotRequired {
        platform: SandboxPlatform,
    },
    Prepared {
        policy: SandboxPolicy,
        platform: SandboxPlatform,
    },
    Unavailable {
        policy: SandboxPolicy,
        platform: SandboxPlatform,
        reason: String,
    },
}
impl SandboxPrepareResult {
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
    pub const fn is_prepared(&self) -> bool {
        matches!(self, Self::Prepared { .. })
    }
    pub const fn allows_spawn_without_false_success(&self) -> bool {
        !self.is_unavailable()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::NotRequired { platform } => {
                format!("sandbox prepare: policy=off not_required platform={platform}")
            }
            Self::Prepared { policy, platform } => {
                format!("sandbox prepare: policy={policy} prepared platform={platform}")
            }
            Self::Unavailable {
                policy,
                platform,
                reason,
            } => format!(
                "sandbox prepare: policy={policy} unavailable platform={platform}: {reason}"
            ),
        }
    }
}

pub fn evaluate_availability(policy: SandboxPolicy) -> SandboxAvailability {
    evaluate_availability_for_platform(policy, current_platform())
}
pub fn evaluate_availability_for_platform(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
) -> SandboxAvailability {
    evaluate_availability_with_landlock(policy, platform, &detect_landlock())
}
pub fn evaluate_availability_with_landlock(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
    _: &LandlockSupport,
) -> SandboxAvailability {
    if policy == SandboxPolicy::Off {
        SandboxAvailability::Available { platform }
    } else {
        SandboxAvailability::Unavailable {
            platform,
            reason: "requires enforcement in the spawned child".into(),
        }
    }
}
pub fn prepare_sandbox(policy: SandboxPolicy) -> SandboxPrepareResult {
    prepare_sandbox_for_platform(policy, current_platform())
}
pub fn prepare_sandbox_for_platform(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
) -> SandboxPrepareResult {
    prepare_sandbox_for_spawn(policy, platform, &detect_landlock(), None, None)
}
pub fn prepare_sandbox_for_spawn(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
    landlock: &LandlockSupport,
    roots: Option<&SandboxPathRoots>,
    apply: Option<&dyn Fn(&SandboxFsPlan) -> Result<(), String>>,
) -> SandboxPrepareResult {
    if policy == SandboxPolicy::Off {
        return SandboxPrepareResult::NotRequired { platform };
    }
    let unavailable = |reason: &str| SandboxPrepareResult::Unavailable {
        policy,
        platform,
        reason: reason.into(),
    };
    if platform != SandboxPlatform::Linux || !landlock.is_available() {
        return unavailable("required Landlock filesystem support is unavailable");
    }
    let Some(plan) = roots.and_then(|roots| build_fs_plan(policy, roots)) else {
        return unavailable("sandbox requires valid filesystem roots");
    };
    let Some(apply) = apply else {
        return unavailable("child enforcement was not supplied");
    };
    match apply(&plan) {
        Ok(()) => SandboxPrepareResult::Prepared { policy, platform },
        Err(_) => unavailable("child filesystem confinement failed"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsSandboxProfile {
    pub policy: SandboxPolicy,
    pub availability: SandboxAvailability,
}
impl OsSandboxProfile {
    pub fn one_line(&self) -> String {
        format!(
            "OS sandbox profile: policy={} {}",
            self.policy,
            self.availability.one_line()
        )
    }
}
pub const OS_SANDBOX_POLICIES: &[SandboxPolicy] = &[
    SandboxPolicy::Off,
    SandboxPolicy::WorkspaceWrite,
    SandboxPolicy::ReadOnly,
    SandboxPolicy::Strict,
];
pub fn list_os_profiles() -> Vec<OsSandboxProfile> {
    list_os_profiles_for_platform(current_platform())
}
pub fn list_os_profiles_for_platform(platform: SandboxPlatform) -> Vec<OsSandboxProfile> {
    let support = detect_landlock();
    OS_SANDBOX_POLICIES
        .iter()
        .map(|&policy| OsSandboxProfile {
            policy,
            availability: evaluate_availability_with_landlock(policy, platform, &support),
        })
        .collect()
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsSandboxProfilesSummary {
    pub total: usize,
    pub available: usize,
    pub unavailable: usize,
}
impl OsSandboxProfilesSummary {
    pub fn one_line(&self) -> String {
        format!(
            "OS sandbox profiles: total={} available={} unavailable={}",
            self.total, self.available, self.unavailable
        )
    }
}
pub fn summarize_os_profiles(profiles: &[OsSandboxProfile]) -> OsSandboxProfilesSummary {
    let available = profiles
        .iter()
        .filter(|p| p.availability.is_available())
        .count();
    OsSandboxProfilesSummary {
        total: profiles.len(),
        available,
        unavailable: profiles.len() - available,
    }
}
