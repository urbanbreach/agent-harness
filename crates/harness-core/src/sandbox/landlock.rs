//! Landlock detection + minimal FS plan (Linux OS sandbox MVP helpers).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::SandboxPolicy;

/// Documented bash child spawn integration site for OS sandbox apply.
///
/// Call order (fail closed): permission allow → `shell_safety` validate →
/// [`super::prepare_sandbox_for_spawn`] (with Landlock apply in child `pre_exec`) →
/// only then spawn. See `crates/harness-tools/src/shell_run.rs`.
pub const BASH_SPAWN_SANDBOX_INTEGRATION: &str =
    "crates/harness-tools/src/shell_run.rs::TokioShellCommandRunner::run";

/// Landlock kernel support (Linux LSM). Presence ≠ confinement applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LandlockSupport {
    /// Landlock LSM is present / enabled for unprivileged use.
    Available { detection: String },
    /// Landlock cannot be used on this host.
    Unavailable { reason: String },
}

impl LandlockSupport {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    /// Operator-facing one-line diagnostics (presence ≠ confinement applied).
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { detection } => {
                format!("Landlock: available ({detection})")
            }
            Self::Unavailable { reason } => {
                format!("Landlock: unavailable ({reason})")
            }
        }
    }
}

/// Workspace-relative path roots used to build a Landlock FS plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPathRoots {
    pub workspace_root: PathBuf,
    pub harness_state_dir: PathBuf,
    pub temp_dir: PathBuf,
}

/// Minimal FS restriction plan for a non-`Off` policy (read/write path roots).
///
/// Network blocking and full profile parity are **not** included (residual).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxFsPlan {
    pub policy: SandboxPolicy,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
}

/// Operator-facing summary of an FS plan (diagnostics only; not enforcement proof).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxFsPlanSummary {
    pub policy: SandboxPolicy,
    pub read_root_count: usize,
    pub write_root_count: usize,
    pub read_roots: Vec<String>,
    pub write_roots: Vec<String>,
}

impl SandboxFsPlanSummary {
    pub fn one_line(&self) -> String {
        format!(
            "policy={} read_roots={} write_roots={}",
            self.policy.as_str(),
            self.read_root_count,
            self.write_root_count
        )
    }
}

/// Summarize a built FS plan for diagnostics / inventory surfaces.
pub fn summarize_fs_plan(plan: &SandboxFsPlan) -> SandboxFsPlanSummary {
    SandboxFsPlanSummary {
        policy: plan.policy,
        read_root_count: plan.read_roots.len(),
        write_root_count: plan.write_roots.len(),
        read_roots: plan
            .read_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        write_roots: plan
            .write_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
    }
}

/// Build + summarize an FS plan for `policy`, or `None` when policy is Off.
pub fn describe_fs_plan_for_policy(
    policy: SandboxPolicy,
    roots: &SandboxPathRoots,
) -> Option<SandboxFsPlanSummary> {
    build_fs_plan(policy, roots).map(|plan| summarize_fs_plan(&plan))
}

/// Detect Landlock on the real host (Linux LSM list). Non-Linux → unavailable.
pub fn detect_landlock() -> LandlockSupport {
    detect_landlock_with(probe_landlock_lsm)
}

/// Injectable Landlock detection for tests.
pub fn detect_landlock_with<F>(probe: F) -> LandlockSupport
where
    F: FnOnce() -> LandlockSupport,
{
    probe()
}

/// Real host probe: read `/sys/kernel/security/lsm` for a `landlock` token.
pub fn probe_landlock_lsm() -> LandlockSupport {
    if !cfg!(target_os = "linux") {
        return LandlockSupport::Unavailable {
            reason: "Landlock is Linux-only; not available on this platform".to_string(),
        };
    }

    match std::fs::read_to_string("/sys/kernel/security/lsm") {
        Ok(contents) => {
            if lsm_list_contains_landlock(&contents) {
                LandlockSupport::Available {
                    detection: "lsm_list".to_string(),
                }
            } else {
                LandlockSupport::Unavailable {
                    reason: format!(
                        "Landlock LSM not enabled in kernel LSM list ({}); \
                         enable CONFIG_SECURITY_LANDLOCK and include landlock in lsm=",
                        contents.trim()
                    ),
                }
            }
        }
        Err(err) => LandlockSupport::Unavailable {
            reason: format!("cannot read /sys/kernel/security/lsm to detect Landlock: {err}"),
        },
    }
}

/// True when the comma-separated LSM list contains the exact token `landlock`.
pub fn lsm_list_contains_landlock(lsm_list: &str) -> bool {
    lsm_list
        .split(',')
        .map(str::trim)
        .any(|token| token == "landlock")
}

/// Build a minimal FS plan for `policy`, or `None` when policy is [`SandboxPolicy::Off`].
pub fn build_fs_plan(policy: SandboxPolicy, roots: &SandboxPathRoots) -> Option<SandboxFsPlan> {
    if !policy.requires_enforcement() {
        return None;
    }

    let system_ro: &[&str] = &["/usr", "/bin", "/lib", "/lib64", "/etc", "/dev", "/proc"];

    let (read_roots, write_roots) = match policy {
        SandboxPolicy::Off => return None,
        SandboxPolicy::WorkspaceWrite => {
            let mut read: Vec<PathBuf> = system_ro.iter().map(PathBuf::from).collect();
            read.push(roots.workspace_root.clone());
            read.push(roots.harness_state_dir.clone());
            read.push(roots.temp_dir.clone());
            let write = vec![
                roots.workspace_root.clone(),
                roots.harness_state_dir.clone(),
                roots.temp_dir.clone(),
            ];
            (read, write)
        }
        SandboxPolicy::ReadOnly => {
            let mut read: Vec<PathBuf> = system_ro.iter().map(PathBuf::from).collect();
            read.push(roots.workspace_root.clone());
            read.push(roots.harness_state_dir.clone());
            read.push(roots.temp_dir.clone());
            let write = vec![roots.harness_state_dir.clone(), roots.temp_dir.clone()];
            (read, write)
        }
        SandboxPolicy::Strict => {
            let read = vec![
                PathBuf::from("/bin"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
                PathBuf::from("/etc"),
                PathBuf::from("/dev"),
                PathBuf::from("/proc"),
                roots.workspace_root.clone(),
                roots.harness_state_dir.clone(),
                roots.temp_dir.clone(),
            ];
            let write = vec![roots.harness_state_dir.clone(), roots.temp_dir.clone()];
            (read, write)
        }
    };

    Some(SandboxFsPlan {
        policy,
        read_roots,
        write_roots,
    })
}

/// Refuse in-process Landlock apply (parent harness must not restrict itself).
///
/// Product probes and parent-process prepare paths use this for fail-closed
/// honesty. Real enforcement is [`apply_landlock_fs_plan`] in child `pre_exec`.
pub fn apply_landlock_fs_plan_not_implemented(plan: &SandboxFsPlan) -> Result<(), String> {
    Err(format!(
        "Landlock FS apply refused in-process for policy `{}` \
         (read_roots={}, write_roots={}); use child pre_exec at {BASH_SPAWN_SANDBOX_INTEGRATION}",
        plan.policy,
        plan.read_roots.len(),
        plan.write_roots.len()
    ))
}

/// Apply a Landlock FS plan via kernel `restrict_self`.
///
/// **Must run only in a child process** (e.g. bash `pre_exec`). Calling from the
/// harness parent restricts the harness itself.
///
/// On non-Linux hosts, returns a structured error (fail closed).
pub fn apply_landlock_fs_plan(plan: &SandboxFsPlan) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        apply_landlock_fs_plan_linux(plan)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = plan;
        Err(format!(
            "Landlock FS apply is Linux-only (policy `{}`); refusing to claim success",
            plan.policy
        ))
    }
}

#[cfg(target_os = "linux")]
fn apply_landlock_fs_plan_linux(plan: &SandboxFsPlan) -> Result<(), String> {
    use landlock::{
        Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, RestrictionStatus, Ruleset,
        RulesetAttr, RulesetCreatedAttr, RulesetStatus, ABI,
    };

    let abi = ABI::V3;
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(abi))
        .map_err(|err| format!("Landlock handle_access failed: {err}"))?
        .create()
        .map_err(|err| format!("Landlock create ruleset failed: {err}"))?;

    let read_access = AccessFs::from_read(abi);
    let write_access = AccessFs::from_all(abi);
    let mut rules_added = 0usize;

    for root in &plan.read_roots {
        // Skip missing optional system roots (e.g. /lib64 on merged-/usr hosts).
        if !root.exists() {
            continue;
        }
        let path_fd = PathFd::new(root).map_err(|err| {
            format!(
                "Landlock PathFd open failed for read root {}: {err}",
                root.display()
            )
        })?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(path_fd, read_access))
            .map_err(|err| {
                format!(
                    "Landlock add_rule read failed for {}: {err}",
                    root.display()
                )
            })?;
        rules_added += 1;
    }

    for root in &plan.write_roots {
        if !root.exists() {
            continue;
        }
        let path_fd = PathFd::new(root).map_err(|err| {
            format!(
                "Landlock PathFd open failed for write root {}: {err}",
                root.display()
            )
        })?;
        ruleset = ruleset
            .add_rule(PathBeneath::new(path_fd, write_access))
            .map_err(|err| {
                format!(
                    "Landlock add_rule write failed for {}: {err}",
                    root.display()
                )
            })?;
        rules_added += 1;
    }

    if rules_added == 0 {
        return Err(format!(
            "Landlock FS plan for policy `{}` had no existing roots to enforce; refusing empty ruleset",
            plan.policy
        ));
    }

    let RestrictionStatus {
        ruleset: status,
        no_new_privs,
        ..
    } = ruleset
        .restrict_self()
        .map_err(|err| format!("Landlock restrict_self failed: {err}"))?;

    match status {
        RulesetStatus::FullyEnforced | RulesetStatus::PartiallyEnforced => {
            if !no_new_privs {
                return Err(
                    "Landlock restrict_self succeeded but no_new_privs was not set; \
                     refusing to claim success"
                        .to_string(),
                );
            }
            Ok(())
        }
        RulesetStatus::NotEnforced => Err(format!(
            "Landlock ruleset not enforced for policy `{}` (kernel lacks support or empty rules); \
             refusing to claim success",
            plan.policy
        )),
    }
}
