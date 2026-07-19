//! Prepare / availability evaluation for OS sandbox policies.

use super::landlock::{
    build_fs_plan, detect_landlock, LandlockSupport, SandboxFsPlan, SandboxPathRoots,
    BASH_SPAWN_SANDBOX_INTEGRATION,
};
use super::{SandboxAvailability, SandboxPlatform, SandboxPolicy, SandboxPrepareResult};

/// Evaluate whether `policy` can be OS-enforced on this host.
pub fn evaluate_availability(policy: SandboxPolicy) -> SandboxAvailability {
    evaluate_availability_for_platform(policy, super::current_platform())
}

/// Platform-injectable availability evaluation (tests + diagnostics).
pub fn evaluate_availability_for_platform(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
) -> SandboxAvailability {
    evaluate_availability_with_landlock(policy, platform, &detect_landlock())
}

/// Availability with injectable Landlock detection (tests).
///
/// Non-`Off` stays unavailable until child apply is wired — Landlock presence
/// alone never becomes silent allow.
pub fn evaluate_availability_with_landlock(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
    landlock: &LandlockSupport,
) -> SandboxAvailability {
    if !policy.requires_enforcement() {
        return SandboxAvailability::Available { platform };
    }

    let reason = unavailable_reason(policy, platform, landlock, EnforceStage::DefaultPath);
    SandboxAvailability::Unavailable { platform, reason }
}

/// Prepare sandbox enforcement for a spawn. Never silently elevates policy.
pub fn prepare_sandbox(policy: SandboxPolicy) -> SandboxPrepareResult {
    prepare_sandbox_for_platform(policy, super::current_platform())
}

/// Platform-injectable prepare (tests + diagnostics). No apply hook.
pub fn prepare_sandbox_for_platform(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
) -> SandboxPrepareResult {
    prepare_sandbox_for_spawn(policy, platform, &detect_landlock(), None, None)
}

/// Prepare sandbox for a concrete spawn with optional path roots and apply hook.
///
/// Fail-closed: non-Linux, missing Landlock, missing roots, missing apply, or
/// apply `Err` → [`SandboxPrepareResult::Unavailable`]. Apply `Ok` → Prepared.
pub fn prepare_sandbox_for_spawn(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
    landlock: &LandlockSupport,
    roots: Option<&SandboxPathRoots>,
    apply: Option<&dyn Fn(&SandboxFsPlan) -> Result<(), String>>,
) -> SandboxPrepareResult {
    if !policy.requires_enforcement() {
        return SandboxPrepareResult::NotRequired { platform };
    }

    if platform != SandboxPlatform::Linux {
        return SandboxPrepareResult::Unavailable {
            policy,
            platform,
            reason: unavailable_reason(policy, platform, landlock, EnforceStage::DefaultPath),
        };
    }

    if landlock.is_unavailable() {
        return SandboxPrepareResult::Unavailable {
            policy,
            platform,
            reason: unavailable_reason(policy, platform, landlock, EnforceStage::NoLandlock),
        };
    }

    let Some(roots) = roots else {
        return SandboxPrepareResult::Unavailable {
            policy,
            platform,
            reason: unavailable_reason(policy, platform, landlock, EnforceStage::MissingRoots),
        };
    };

    let Some(plan) = build_fs_plan(policy, roots) else {
        return SandboxPrepareResult::NotRequired { platform };
    };

    let Some(apply) = apply else {
        return SandboxPrepareResult::Unavailable {
            policy,
            platform,
            reason: unavailable_reason(policy, platform, landlock, EnforceStage::ApplyNotWired),
        };
    };

    match apply(&plan) {
        Ok(()) => SandboxPrepareResult::Prepared { policy, platform },
        Err(err) => SandboxPrepareResult::Unavailable {
            policy,
            platform,
            reason: format!(
                "Landlock apply failed for OS sandbox profile `{policy}`: {err}; \
                 refusing to claim success"
            ),
        },
    }
}

#[derive(Debug, Clone, Copy)]
enum EnforceStage {
    DefaultPath,
    NoLandlock,
    MissingRoots,
    ApplyNotWired,
}

fn unavailable_reason(
    policy: SandboxPolicy,
    platform: SandboxPlatform,
    landlock: &LandlockSupport,
    stage: EnforceStage,
) -> String {
    match platform {
        SandboxPlatform::Linux => match stage {
            EnforceStage::NoLandlock => match landlock {
                LandlockSupport::Unavailable { reason } => format!(
                    "OS sandbox profile `{policy}` is not enforceable on linux: {reason}; \
                     refusing to claim success"
                ),
                LandlockSupport::Available { .. } => format!(
                    "OS sandbox profile `{policy}` is not enforceable on linux \
                     (Landlock missing); refusing to claim success"
                ),
            },
            EnforceStage::MissingRoots => format!(
                "OS sandbox profile `{policy}` requires SandboxPathRoots for Landlock FS plan; \
                 refusing to claim success"
            ),
            EnforceStage::ApplyNotWired => format!(
                "OS sandbox profile `{policy}`: Landlock LSM present but bash child pre_exec \
                 apply is not wired at {BASH_SPAWN_SANDBOX_INTEGRATION}; refusing to claim success"
            ),
            EnforceStage::DefaultPath => match landlock {
                LandlockSupport::Available { detection } => format!(
                    "OS sandbox profile `{policy}` is not enforceable yet on linux \
                     (Landlock detected via {detection}; child pre_exec apply not wired at \
                     {BASH_SPAWN_SANDBOX_INTEGRATION}); refusing to claim success"
                ),
                LandlockSupport::Unavailable { reason } => format!(
                    "OS sandbox profile `{policy}` is not enforceable on linux: {reason}; \
                     refusing to claim success"
                ),
            },
        },
        SandboxPlatform::Macos => format!(
            "OS sandbox profile `{policy}` is not enforceable on macos yet \
             (Seatbelt backend not implemented); refusing to claim success"
        ),
        SandboxPlatform::Windows => format!(
            "OS sandbox profile `{policy}` is not enforceable on windows yet \
             (Job Object / AppContainer backend not implemented); refusing to claim success"
        ),
        SandboxPlatform::Other => format!(
            "OS sandbox profile `{policy}` is not enforceable on {platform} yet \
             (no OS sandbox backend); refusing to claim success"
        ),
    }
}
