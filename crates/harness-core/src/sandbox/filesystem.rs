use super::SandboxPolicy;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const BASH_SPAWN_SANDBOX_INTEGRATION: &str = "harness child-process confinement";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LandlockSupport {
    Available { detection: String },
    Unavailable { reason: String },
}
impl LandlockSupport {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
    pub const fn is_unavailable(&self) -> bool {
        !self.is_available()
    }
    pub fn one_line(&self) -> String {
        match self {
            Self::Available { detection } => format!("Landlock: available {detection}"),
            Self::Unavailable { reason } => format!("Landlock: unavailable {reason}"),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPathRoots {
    pub workspace_root: PathBuf,
    pub harness_state_dir: PathBuf,
    pub temp_dir: PathBuf,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxFsPlan {
    pub policy: SandboxPolicy,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
}
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
            self.policy, self.read_root_count, self.write_root_count
        )
    }
}
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
pub fn describe_fs_plan_for_policy(
    policy: SandboxPolicy,
    roots: &SandboxPathRoots,
) -> Option<SandboxFsPlanSummary> {
    build_fs_plan(policy, roots).as_ref().map(summarize_fs_plan)
}
pub fn detect_landlock_with<F: FnOnce() -> LandlockSupport>(probe: F) -> LandlockSupport {
    probe()
}
pub fn probe_landlock_lsm() -> LandlockSupport {
    detect_landlock()
}
pub fn lsm_list_contains_landlock(list: &str) -> bool {
    list.split(',').any(|s| s.trim() == "landlock")
}
pub fn detect_landlock() -> LandlockSupport {
    #[cfg(target_os = "linux")]
    if linux_ruleset().is_ok() {
        return LandlockSupport::Available {
            detection: "filesystem ABI 5 ruleset creation; not enforcement".into(),
        };
    }
    LandlockSupport::Unavailable {
        reason: "filesystem ABI 5 ruleset creation is unavailable".into(),
    }
}
pub fn build_fs_plan(policy: SandboxPolicy, roots: &SandboxPathRoots) -> Option<SandboxFsPlan> {
    let paths = [
        &roots.workspace_root,
        &roots.harness_state_dir,
        &roots.temp_dir,
    ];
    if policy == SandboxPolicy::Off || paths.iter().any(|p| !absolute_normal(p)) {
        return None;
    }
    let mut read_roots: Vec<PathBuf> = ["/usr", "/bin", "/lib", "/lib64", "/etc", "/dev", "/proc"]
        .into_iter()
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();
    // A shared temp ancestor would expose unrelated projects to a read-only worker too.
    read_roots.extend(
        paths
            .into_iter()
            .filter(|p| *p == &roots.workspace_root || !roots.workspace_root.starts_with(p))
            .cloned(),
    );
    let mut write_roots = vec![];
    if policy == SandboxPolicy::WorkspaceWrite {
        write_roots.push(roots.workspace_root.clone());
    }
    write_roots.extend(
        [&roots.harness_state_dir, &roots.temp_dir]
            .into_iter()
            .filter(|p| !roots.workspace_root.starts_with(p))
            .cloned(),
    );
    for special in ["/dev/null", "/dev/zero"] {
        if Path::new(special).exists() {
            write_roots.push(special.into());
        }
    }
    read_roots.sort();
    read_roots.dedup();
    write_roots.sort();
    write_roots.dedup();
    Some(SandboxFsPlan {
        policy,
        read_roots,
        write_roots,
    })
}
fn absolute_normal(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}
pub fn apply_landlock_fs_plan_not_implemented(_: &SandboxFsPlan) -> Result<(), String> {
    Err("plan inspection does not apply filesystem confinement".into())
}
/// Irreversible: call only in the child or a dedicated worker that exits afterward.
/// Restricts filesystem access; it does not isolate networking or other processes.
pub fn apply_landlock_fs_plan(plan: &SandboxFsPlan) -> Result<(), String> {
    if plan.policy == SandboxPolicy::Off {
        return Ok(());
    }
    if plan.read_roots.is_empty() || plan.read_roots.len() + plan.write_roots.len() > 64 {
        return Err("invalid filesystem root inventory".into());
    }
    for path in plan.read_roots.iter().chain(&plan.write_roots) {
        if !absolute_normal(path) {
            return Err("filesystem roots must be absolute".into());
        }
    }
    for path in &plan.write_roots {
        crate::store::validate_private_path(path).map_err(|_| "write root contains a symlink")?;
    }
    #[cfg(target_os = "linux")]
    {
        linux_apply(plan).map_err(|_| "Landlock filesystem confinement failed".into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("Landlock requires Linux".into())
    }
}

#[cfg(target_os = "linux")]
fn linux_ruleset() -> Result<landlock::RulesetCreated, landlock::RulesetError> {
    use landlock::{Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V5))?
        .create()
}
#[cfg(target_os = "linux")]
fn linux_apply(plan: &SandboxFsPlan) -> Result<(), Box<dyn std::error::Error>> {
    use landlock::{path_beneath_rules, Access, AccessFs, RulesetCreatedAttr, RulesetStatus, ABI};
    let status = linux_ruleset()?
        .add_rules(path_beneath_rules(
            &plan.read_roots,
            AccessFs::from_read(ABI::V5),
        ))?
        .add_rules(path_beneath_rules(
            &plan.write_roots,
            AccessFs::from_all(ABI::V5),
        ))?
        .restrict_self()?;
    if status.ruleset != RulesetStatus::FullyEnforced || !status.no_new_privs {
        return Err("filesystem confinement was not fully enforced".into());
    }
    Ok(())
}
