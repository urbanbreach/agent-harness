//! Typed network confinement policies backed by Linux Landlock TCP-port rules.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{LandlockSupport, SandboxPlatform};

/// Network policy for a sandboxed child process.
///
/// Linux Landlock V4 can constrain TCP ports but cannot constrain peer addresses
/// or CIDRs. This type intentionally exposes only that enforceable coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SandboxNetworkPolicy {
    /// Do not add network restrictions to the child Landlock ruleset.
    Unrestricted,
    /// Deny all TCP connect and bind operations in the child.
    DenyAll,
    /// Allow outbound TCP connections only to these destination ports.
    AllowTcpPorts { allowed_ports: BTreeSet<u16> },
}

impl SandboxNetworkPolicy {
    /// Parse the shell configuration form: `deny`, `unrestricted`, or `tcp:<ports>`.
    pub fn parse(value: &str) -> Result<Self, SandboxNetworkPolicyError> {
        let value = value.trim();
        match value {
            "deny" => Ok(Self::DenyAll),
            "unrestricted" => Ok(Self::Unrestricted),
            _ => Self::parse_tcp_ports(value),
        }
    }

    fn parse_tcp_ports(value: &str) -> Result<Self, SandboxNetworkPolicyError> {
        let Some(ports) = value.strip_prefix("tcp:") else {
            return Err(SandboxNetworkPolicyError::UnknownPolicy {
                value: value.to_string(),
            });
        };
        if ports.is_empty() {
            return Err(SandboxNetworkPolicyError::EmptyPortList);
        }

        let mut allowed_ports = BTreeSet::new();
        for raw_port in ports.split(',') {
            let port = raw_port.trim().parse::<u16>().map_err(|_| {
                SandboxNetworkPolicyError::InvalidPort {
                    value: raw_port.trim().to_string(),
                }
            })?;
            if port == 0 {
                return Err(SandboxNetworkPolicyError::InvalidPort {
                    value: raw_port.trim().to_string(),
                });
            }
            allowed_ports.insert(port);
        }

        Ok(Self::AllowTcpPorts { allowed_ports })
    }

    /// True when this policy requires a Landlock network ruleset.
    pub const fn requires_enforcement(&self) -> bool {
        !matches!(self, Self::Unrestricted)
    }
}

/// Invalid public network policy input.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SandboxNetworkPolicyError {
    #[error("unknown sandbox network policy: {value}")]
    UnknownPolicy { value: String },
    #[error("sandbox network TCP policy requires at least one port")]
    EmptyPortList,
    #[error("invalid sandbox network TCP port: {value}")]
    InvalidPort { value: String },
}

/// Truthful network-confinement readiness for a requested policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NetworkConfinementStatus {
    /// The policy does not ask the kernel to restrict networking.
    NotRequired { platform: SandboxPlatform },
    /// Landlock V4 network rules are available for child-only application.
    Available { platform: SandboxPlatform },
    /// Requested network rules cannot be enforced on this host.
    Unavailable {
        platform: SandboxPlatform,
        reason: String,
    },
}

impl NetworkConfinementStatus {
    /// True when a child can be launched without misreporting enforcement.
    pub const fn allows_spawn_without_false_success(&self) -> bool {
        matches!(self, Self::NotRequired { .. } | Self::Available { .. })
    }
}

/// Evaluate network confinement against the current host.
pub fn evaluate_network_confinement(policy: &SandboxNetworkPolicy) -> NetworkConfinementStatus {
    evaluate_network_confinement_with_landlock(
        policy,
        super::current_platform(),
        &super::detect_landlock(),
    )
}

/// Evaluate network confinement with injectable platform and Landlock support.
pub fn evaluate_network_confinement_with_landlock(
    policy: &SandboxNetworkPolicy,
    platform: SandboxPlatform,
    landlock: &LandlockSupport,
) -> NetworkConfinementStatus {
    if !policy.requires_enforcement() {
        return NetworkConfinementStatus::NotRequired { platform };
    }
    if platform != SandboxPlatform::Linux {
        return NetworkConfinementStatus::Unavailable {
            platform,
            reason: format!(
                "sandbox network confinement is Linux Landlock V4-only; {platform} has no backend"
            ),
        };
    }
    match landlock {
        LandlockSupport::Unavailable { reason } => NetworkConfinementStatus::Unavailable {
            platform,
            reason: format!(
                "sandbox network confinement requires Landlock V4: {reason}; refusing to claim success"
            ),
        },
        LandlockSupport::Available { .. } => probe_landlock_network_support(platform),
    }
}

#[cfg(target_os = "linux")]
fn probe_landlock_network_support(platform: SandboxPlatform) -> NetworkConfinementStatus {
    use landlock::{Access, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};

    let result = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessNet::from_all(ABI::V4))
        .and_then(|ruleset| ruleset.create());
    match result {
        Ok(_) => NetworkConfinementStatus::Available { platform },
        Err(error) => NetworkConfinementStatus::Unavailable {
            platform,
            reason: format!(
                "sandbox network confinement requires Landlock ABI V4 TCP rules: {error}; refusing to claim success"
            ),
        },
    }
}

#[cfg(not(target_os = "linux"))]
fn probe_landlock_network_support(platform: SandboxPlatform) -> NetworkConfinementStatus {
    NetworkConfinementStatus::Unavailable {
        platform,
        reason: "sandbox network confinement is Linux Landlock V4-only; refusing to claim success"
            .to_string(),
    }
}
