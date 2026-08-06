use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use harness_testkit::tui_fidelity_deadline::{
    CommandSpec, CommandStatus, DeadlineRunner, InterruptFlag, ResourceLimits,
};
use harness_testkit::tui_fidelity_dependency_cone::{parse_git_changes, DependencyCones};
use harness_testkit::tui_fidelity_matrix::read_coverage_documents;
use harness_testkit::tui_fidelity_verify::{
    build_plan, execute_plan, PlanSelection, VerificationProfile, VerifyConfig,
};

use super::verify_executor::VerifyExecutor;

pub(super) fn execute(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse(arguments, repo_root)?;
    let interrupt = InterruptFlag::install().map_err(|error| error.to_string())?;
    let git = DeadlineRunner::new(
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(1),
        ResourceLimits::unrestricted(),
        interrupt.clone(),
    );
    let base_sha = git_text(
        &git,
        CommandSpec::new("git")
            .args(["rev-parse", "--verify"])
            .args([format!("{}^{{commit}}", args.base_sha)])
            .cwd(repo_root),
    )?;
    let candidate_sha = git_text(
        &git,
        CommandSpec::new("git")
            .args(["rev-parse", "--verify"])
            .args([format!("{}^{{commit}}", args.head_sha)])
            .cwd(repo_root),
    )?;
    let (inventory, manifest, _) = read_coverage_documents(&args.inventory, &args.manifest)
        .map_err(|error| error.to_string())?;
    let changed = if args.profile == VerificationProfile::Changed {
        let tracked = git_output(
            &git,
            CommandSpec::new("git")
                .args(["diff", "--name-status", "-z"])
                .args([base_sha.as_str(), candidate_sha.as_str()])
                .cwd(repo_root),
        )?;
        let untracked = git_output(
            &git,
            CommandSpec::new("git")
                .args(["ls-files", "--others", "--exclude-standard", "-z"])
                .cwd(repo_root),
        )?;
        let paths = parse_git_changes(&tracked, &untracked).map_err(|error| error.to_string())?;
        let input = fs::read_to_string(&args.cones)
            .map_err(|error| format!("{}: {error}", args.cones.display()))?;
        let cones = DependencyCones::from_json(&input).map_err(|error| error.to_string())?;
        Some(
            cones
                .select(
                    &paths.into_iter().map(PathBuf::from).collect::<Vec<_>>(),
                    &inventory,
                    &manifest,
                )
                .map_err(|error| error.to_string())?
                .requirement_ids,
        )
    } else {
        None
    };
    let plan = build_plan(
        PlanSelection {
            profile: args.profile,
            changed: changed.as_ref(),
        },
        &inventory,
        &manifest,
    )
    .map_err(|error| error.to_string())?;
    let executor = VerifyExecutor::new(repo_root, &args, interrupt)
        .map_err(|error| format!("verify executor: {error}"))?;
    let receipt = execute_plan(
        &VerifyConfig {
            candidate_sha,
            attempt_id: args.attempt_id,
            evidence_root: args.evidence_root,
            workers: args.workers,
        },
        &plan,
        |key, isolation| executor.execute(key, isolation),
    )
    .map_err(|error| error.to_string())?;
    println!(
        "tui-fidelity verify PASS: {}, {} obligations, {} keys, {} ms, evidence {}",
        receipt.profile,
        receipt.obligation_count,
        receipt.key_count,
        receipt.duration_millis,
        receipt.evidence_path
    );
    Ok(())
}

pub(super) struct VerifyArgs {
    pub profile: VerificationProfile,
    pub base_sha: String,
    pub head_sha: String,
    pub inventory: PathBuf,
    pub manifest: PathBuf,
    pub cones: PathBuf,
    pub evidence_root: PathBuf,
    pub reference_bin: PathBuf,
    pub harness_bin: PathBuf,
    pub candidate_receipt: PathBuf,
    pub browser_bin: Option<PathBuf>,
    pub node_modules: Option<PathBuf>,
    pub font_family: String,
    pub attempt_id: String,
    pub workers: Option<usize>,
}

fn parse(arguments: Vec<OsString>, repo_root: &Path) -> Result<VerifyArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new("verify")) {
        return Err(
            "usage: verify --profile changed|all|motion --base-sha SHA --head-sha SHA --reference-bin PATH --harness-bin PATH".to_owned(),
        );
    }
    let mut profile = None;
    let mut base_sha = None;
    let mut head_sha = None;
    let mut inventory = repo_root.join("configs/tui-fidelity-requirement-inventory.json");
    let mut manifest = repo_root.join("configs/tui-fidelity-coverage-manifest.json");
    let mut cones = repo_root.join("configs/tui-fidelity-dependency-cones.json");
    let mut evidence_root = repo_root.join(".omo/evidence");
    let mut reference_bin = None;
    let mut harness_bin = None;
    let mut candidate_receipt = None;
    let mut browser_bin = None;
    let mut node_modules = None;
    let mut font_family = "DejaVu Sans Mono".to_owned();
    let mut attempt_id = None;
    let mut workers = None;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--profile") => {
                profile = Some(value.to_string_lossy().parse().map_err(
                    |error: harness_testkit::tui_fidelity_verify::VerifyError| error.to_string(),
                )?)
            }
            Some("--base-sha") => base_sha = Some(value.to_string_lossy().into_owned()),
            Some("--head-sha") => head_sha = Some(value.to_string_lossy().into_owned()),
            Some("--inventory") => inventory = value.into(),
            Some("--manifest") => manifest = value.into(),
            Some("--dependency-cones") => cones = value.into(),
            Some("--evidence-root") => evidence_root = value.into(),
            Some("--reference-bin") => reference_bin = Some(value.into()),
            Some("--harness-bin") => harness_bin = Some(value.into()),
            Some("--candidate-receipt") => candidate_receipt = Some(value.into()),
            Some("--browser-bin") => browser_bin = Some(value.into()),
            Some("--node-modules") => node_modules = Some(value.into()),
            Some("--font-family") => font_family = value.to_string_lossy().into_owned(),
            Some("--attempt-id") => attempt_id = Some(value.to_string_lossy().into_owned()),
            Some("--workers") => {
                workers = Some(
                    value
                        .to_string_lossy()
                        .parse()
                        .map_err(|error| format!("invalid workers: {error}"))?,
                )
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    let default_attempt = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis();
    Ok(VerifyArgs {
        profile: profile.ok_or("missing --profile")?,
        base_sha: base_sha.ok_or("missing --base-sha")?,
        head_sha: head_sha.ok_or("missing --head-sha")?,
        inventory,
        manifest,
        cones,
        evidence_root,
        reference_bin: reference_bin.ok_or("missing --reference-bin")?,
        harness_bin: harness_bin.ok_or("missing --harness-bin")?,
        candidate_receipt: candidate_receipt.ok_or("missing --candidate-receipt")?,
        browser_bin,
        node_modules,
        font_family,
        attempt_id: attempt_id
            .unwrap_or_else(|| format!("{}-{default_attempt}", std::process::id())),
        workers,
    })
}

fn git_output(runner: &DeadlineRunner, command: CommandSpec) -> Result<Vec<u8>, String> {
    let receipt = runner.run(&command).map_err(|error| error.to_string())?;
    if receipt.status == CommandStatus::Passed {
        Ok(receipt.stdout.into_bytes())
    } else {
        Err(format!(
            "Git command {:?}: {}",
            receipt.status, receipt.stderr
        ))
    }
}

fn git_text(runner: &DeadlineRunner, command: CommandSpec) -> Result<String, String> {
    String::from_utf8(git_output(runner, command)?)
        .map(|value| value.trim().to_owned())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_parser_accepts_all_typed_profiles_with_revision_bounds() {
        for profile in ["changed", "all", "motion"] {
            let arguments = vec![
                OsString::from("verify"),
                OsString::from("--profile"),
                OsString::from(profile),
                OsString::from("--base-sha"),
                OsString::from("base"),
                OsString::from("--head-sha"),
                OsString::from("head"),
                OsString::from("--reference-bin"),
                OsString::from("reference"),
                OsString::from("--harness-bin"),
                OsString::from("harness"),
                OsString::from("--candidate-receipt"),
                OsString::from("candidate-receipt"),
            ];

            let parsed = parse(arguments, Path::new("/repo")).expect("typed profile arguments");
            assert_eq!(parsed.base_sha, "base");
            assert_eq!(parsed.head_sha, "head");
            assert_eq!(parsed.profile.to_string(), profile);
        }
    }
}
