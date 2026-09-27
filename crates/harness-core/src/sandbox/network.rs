use super::{current_platform, detect_landlock, LandlockSupport, SandboxPlatform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SandboxNetworkPolicy {
    Unrestricted,
    DenyAll,
    AllowTcpPorts { allowed_ports: BTreeSet<u16> },
}
#[derive(Debug, thiserror::Error)]
pub enum SandboxNetworkPolicyError {
    #[error("unknown sandbox network policy: {value}")]
    UnknownPolicy { value: String },
    #[error("sandbox network TCP policy requires at least one port")]
    EmptyPortList,
    #[error("invalid sandbox network TCP port: {value}")]
    InvalidPort { value: String },
}
impl SandboxNetworkPolicy {
    pub fn parse(value: &str) -> Result<Self, SandboxNetworkPolicyError> {
        match value.trim() {
            "unrestricted" => Ok(Self::Unrestricted),
            "deny" => Ok(Self::DenyAll),
            value if value.starts_with("tcp:") => {
                let list = &value[4..];
                if list.is_empty() {
                    return Err(SandboxNetworkPolicyError::EmptyPortList);
                }
                let allowed_ports = list
                    .split(',')
                    .map(|s| {
                        s.trim()
                            .parse::<u16>()
                            .ok()
                            .filter(|p| *p != 0)
                            .ok_or_else(|| SandboxNetworkPolicyError::InvalidPort {
                                value: s.into(),
                            })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Self::AllowTcpPorts { allowed_ports })
            }
            _ => Err(SandboxNetworkPolicyError::UnknownPolicy {
                value: value.into(),
            }),
        }
    }
    pub const fn is_unrestricted(&self) -> bool {
        matches!(self, Self::Unrestricted)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NetworkConfinementStatus {
    NotRequired {
        platform: SandboxPlatform,
    },
    Available {
        platform: SandboxPlatform,
    },
    Unavailable {
        platform: SandboxPlatform,
        reason: String,
    },
}
impl NetworkConfinementStatus {
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }
    pub const fn allows_spawn_without_false_success(&self) -> bool {
        matches!(self, Self::NotRequired { .. })
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::NotRequired { platform } => {
                format!("network confinement: not_required platform={platform}")
            }
            Self::Available { platform } => format!(
                "network confinement: available platform={platform}; application still required"
            ),
            Self::Unavailable { platform, reason } => {
                format!("network confinement: unavailable platform={platform}: {reason}")
            }
        }
    }
}
pub fn evaluate_network_confinement(policy: &SandboxNetworkPolicy) -> NetworkConfinementStatus {
    evaluate_network_confinement_with_landlock(policy, current_platform(), &detect_landlock())
}
pub fn evaluate_network_confinement_with_landlock(
    policy: &SandboxNetworkPolicy,
    platform: SandboxPlatform,
    _: &LandlockSupport,
) -> NetworkConfinementStatus {
    if policy.is_unrestricted() {
        return NetworkConfinementStatus::NotRequired { platform };
    }
    // Landlock filters TCP, not UDP or Unix sockets. A deny-all claim would be false.
    NetworkConfinementStatus::Unavailable {
        platform,
        reason: "network enforcement is not connected to the child runner".into(),
    }
}
