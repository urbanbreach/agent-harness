use super::*;

pub const OS_SANDBOX_ENFORCED_POLICIES: &[SandboxPolicy] = &[
    SandboxPolicy::WorkspaceWrite,
    SandboxPolicy::ReadOnly,
    SandboxPolicy::Strict,
];
#[derive(Debug, Clone)]
pub struct OsSandboxProductProbe {
    pub landlock: LandlockSupport,
    pub platform: SandboxPlatform,
    pub profiles: Vec<OsSandboxProfile>,
    pub profiles_summary: OsSandboxProfilesSummary,
    pub prepare_results: Vec<SandboxPrepareResult>,
    pub last_prepare: SandboxPrepareResult,
    pub fs_plan_summaries: Vec<SandboxFsPlanSummary>,
    pub last_fs_plan: Option<SandboxFsPlanSummary>,
    pub apply_honesty: Vec<String>,
}
impl OsSandboxProductProbe {
    pub fn non_off_prepare_all_unavailable(&self) -> bool {
        self.prepare_results
            .iter()
            .all(SandboxPrepareResult::is_unavailable)
    }
    pub fn one_line(&self) -> String {
        format!(
            "{}; {}; {}",
            self.landlock.one_line(),
            self.profiles_summary.one_line(),
            self.last_prepare.one_line()
        )
    }
}
pub fn probe_os_sandbox_product(roots: Option<&SandboxPathRoots>) -> OsSandboxProductProbe {
    probe_os_sandbox_product_for_platform(current_platform(), roots)
}
pub fn probe_os_sandbox_product_for_platform(
    platform: SandboxPlatform,
    roots: Option<&SandboxPathRoots>,
) -> OsSandboxProductProbe {
    let landlock = detect_landlock();
    let profiles: Vec<_> = OS_SANDBOX_POLICIES
        .iter()
        .map(|&policy| OsSandboxProfile {
            policy,
            availability: evaluate_availability_with_landlock(policy, platform, &landlock),
        })
        .collect();
    let prepare_results: Vec<_> = OS_SANDBOX_ENFORCED_POLICIES
        .iter()
        .map(|&policy| prepare_sandbox_for_spawn(policy, platform, &landlock, roots, None))
        .collect();
    let fs_plan_summaries: Vec<_> = roots
        .into_iter()
        .flat_map(|roots| {
            OS_SANDBOX_ENFORCED_POLICIES
                .iter()
                .filter_map(|&policy| describe_fs_plan_for_policy(policy, roots))
        })
        .collect();
    OsSandboxProductProbe {
        profiles_summary: summarize_os_profiles(&profiles),
        last_prepare: prepare_sandbox_for_spawn(
            SandboxPolicy::Strict,
            platform,
            &landlock,
            roots,
            None,
        ),
        last_fs_plan: fs_plan_summaries.last().cloned(),
        apply_honesty: vec!["profile inspection does not confine a process".into()],
        landlock,
        platform,
        profiles,
        prepare_results,
        fs_plan_summaries,
    }
}
