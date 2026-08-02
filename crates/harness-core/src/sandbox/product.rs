//! Multi-policy OS sandbox product path (detect → list → prepare → FS plan walk).
//!
//! Honesty: this probe never applies Landlock in-process. Child confinement is
//! [`super::apply_landlock_fs_plan`] via bash `pre_exec` when
//! `HARNESS_OS_SANDBOX_POLICY` is non-Off (default Off). Parent probes use
//! [`super::apply_landlock_fs_plan_not_implemented`].

use super::landlock::{
    apply_landlock_fs_plan_not_implemented, build_fs_plan, describe_fs_plan_for_policy,
    detect_landlock, LandlockSupport, SandboxFsPlanSummary, SandboxPathRoots,
};
use super::prepare::prepare_sandbox_for_platform;
use super::{
    current_platform, list_os_profiles_for_platform, summarize_os_profiles, OsSandboxProfile,
    OsSandboxProfilesSummary, SandboxPlatform, SandboxPolicy, SandboxPrepareResult,
    OS_SANDBOX_POLICIES,
};

/// Policies walked for multi-policy FS plan product path (enforced set only).
pub const OS_SANDBOX_ENFORCED_POLICIES: &[SandboxPolicy] = &[
    SandboxPolicy::WorkspaceWrite,
    SandboxPolicy::ReadOnly,
    SandboxPolicy::Strict,
];

/// Product result of the multi-policy OS sandbox surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsSandboxProductProbe {
    pub landlock: LandlockSupport,
    pub platform: SandboxPlatform,
    pub profiles: Vec<OsSandboxProfile>,
    pub profiles_summary: OsSandboxProfilesSummary,
    pub prepare_results: Vec<SandboxPrepareResult>,
    pub last_prepare: SandboxPrepareResult,
    pub fs_plan_summaries: Vec<SandboxFsPlanSummary>,
    pub last_fs_plan: Option<SandboxFsPlanSummary>,
    /// One honesty reason per enforced plan (in-process apply refused).
    pub apply_honesty: Vec<String>,
}

impl OsSandboxProductProbe {
    /// True when every non-`Off` prepare is structured unavailable (no silent allow).
    pub fn non_off_prepare_all_unavailable(&self) -> bool {
        self.prepare_results.iter().all(|result| match result {
            SandboxPrepareResult::NotRequired { .. } => true,
            SandboxPrepareResult::Unavailable { .. } => true,
            SandboxPrepareResult::Prepared { .. } => false,
        })
    }

    pub fn one_line(&self) -> String {
        let last = self
            .last_fs_plan
            .as_ref()
            .map(|s| s.policy.as_str())
            .unwrap_or("none");
        format!(
            "OS sandbox product: profiles={} prepare={} fs_plans={} last_plan={} landlock={}",
            self.profiles_summary.total,
            self.prepare_results.len(),
            self.fs_plan_summaries.len(),
            last,
            if self.landlock.is_available() {
                "available"
            } else {
                "unavailable"
            }
        )
    }
}

/// Product path: detect Landlock, list all OS profiles, prepare every public policy,
/// walk WorkspaceWrite→ReadOnly→Strict FS plans with apply-not-implemented honesty.
///
/// When `roots` is `None`, FS plan walk is skipped (`last_fs_plan = None`).
pub fn probe_os_sandbox_product(roots: Option<&SandboxPathRoots>) -> OsSandboxProductProbe {
    probe_os_sandbox_product_for_platform(current_platform(), roots)
}

/// Platform-injectable product path (tests + diagnostics).
pub fn probe_os_sandbox_product_for_platform(
    platform: SandboxPlatform,
    roots: Option<&SandboxPathRoots>,
) -> OsSandboxProductProbe {
    let landlock = detect_landlock();
    let profiles = list_os_profiles_for_platform(platform);
    let profiles_summary = summarize_os_profiles(&profiles);

    let mut prepare_results = Vec::with_capacity(OS_SANDBOX_POLICIES.len());
    for &policy in OS_SANDBOX_POLICIES {
        prepare_results.push(prepare_sandbox_for_platform(policy, platform));
    }
    let last_prepare = prepare_results
        .last()
        .cloned()
        .unwrap_or(SandboxPrepareResult::NotRequired { platform });

    let mut fs_plan_summaries = Vec::new();
    let mut apply_honesty = Vec::new();
    if let Some(roots) = roots {
        for &policy in OS_SANDBOX_ENFORCED_POLICIES {
            if let Some(summary) = describe_fs_plan_for_policy(policy, roots) {
                if let Some(plan) = build_fs_plan(policy, roots) {
                    match apply_landlock_fs_plan_not_implemented(&plan) {
                        Ok(()) => apply_honesty.push(format!(
                            "unexpected apply success for policy={}",
                            policy.as_str()
                        )),
                        Err(reason) => apply_honesty.push(reason),
                    }
                }
                fs_plan_summaries.push(summary);
            }
        }
    }
    let last_fs_plan = fs_plan_summaries.last().cloned();

    OsSandboxProductProbe {
        landlock,
        platform,
        profiles,
        profiles_summary,
        prepare_results,
        last_prepare,
        fs_plan_summaries,
        last_fs_plan,
        apply_honesty,
    }
}
