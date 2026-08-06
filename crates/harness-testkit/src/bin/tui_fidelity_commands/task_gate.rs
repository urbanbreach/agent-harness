use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_testkit::tui_fidelity_task_gate::{self, TaskGateInput};

pub(super) fn execute_admit(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse(arguments, "task-admit", repo_root)?;
    let path = tui_fidelity_task_gate::admit(&args.input).map_err(|error| error.to_string())?;
    println!("tui-fidelity task-admit PASS: {}", path.display());
    Ok(())
}

pub(super) fn execute_verify(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse(arguments, "task-verify", repo_root)?;
    let path = tui_fidelity_task_gate::verify(&args.input).map_err(|error| error.to_string())?;
    println!("tui-fidelity task-verify PASS: {}", path.display());
    Ok(())
}

pub(super) fn execute_complete(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse(arguments, "task-complete", repo_root)?;
    let path = tui_fidelity_task_gate::complete(&args.input).map_err(|error| error.to_string())?;
    println!("tui-fidelity task-complete PASS: {}", path.display());
    Ok(())
}

struct ParsedArgs {
    input: TaskGateInput,
}

fn parse(
    arguments: Vec<OsString>,
    command_name: &str,
    repo_root: &Path,
) -> Result<ParsedArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new(command_name)) {
        return Err(format!("usage: {command_name} TASK [options]"));
    }
    let task = values
        .next()
        .ok_or_else(|| format!("{command_name} requires a task label"))?
        .to_string_lossy()
        .into_owned();
    let evidence_root = repo_root
        .join(".omo/evidence")
        .join(format!("task-{task}-grok-build-tui-experiential-parity"));
    let mut boulder = repo_root.join(".omo/boulder.json");
    let mut plan = repo_root.join(".omo/plans/grok-build-tui-experiential-parity.md");
    let mut candidate_sha256 = current_head(repo_root)?;
    let mut evidence_root_value = evidence_root.clone();
    let mut admission_receipt = evidence_root.join(format!("task-admission-{task}.json"));
    let mut aggregate_receipt = evidence_root.join("watchdog-receipt.json");
    let mut verification_receipt = evidence_root.join("verification-receipt.json");
    let mut gate_receipt = evidence_root.join(format!("task-verification-{task}.json"));
    let mut completion_receipt = evidence_root.join(format!("task-completion-{task}.json"));
    let mut revocations = Vec::new();
    let mut shards = Vec::new();
    let mut dependencies = Vec::new();
    let mut finalize_closure = false;
    let mut closure_receipt = None;
    while let Some(flag) = values.next() {
        match flag.to_str() {
            Some("--finalize-closure") => finalize_closure = true,
            Some(
                "--boulder"
                | "--plan"
                | "--candidate-sha"
                | "--evidence-root"
                | "--admission"
                | "--aggregate"
                | "--verification"
                | "--receipt"
                | "--closure-receipt"
                | "--revocation"
                | "--shard"
                | "--dependency-receipt",
            ) => {
                let value = values
                    .next()
                    .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
                match flag.to_str() {
                    Some("--boulder") => boulder = PathBuf::from(value),
                    Some("--plan") => plan = PathBuf::from(value),
                    Some("--candidate-sha") => {
                        candidate_sha256 = value.to_string_lossy().into_owned()
                    }
                    Some("--evidence-root") => {
                        evidence_root_value = PathBuf::from(&value);
                        admission_receipt =
                            evidence_root_value.join(format!("task-admission-{task}.json"));
                        aggregate_receipt = evidence_root_value.join("watchdog-receipt.json");
                        verification_receipt =
                            evidence_root_value.join("verification-receipt.json");
                        gate_receipt =
                            evidence_root_value.join(format!("task-verification-{task}.json"));
                        completion_receipt =
                            evidence_root_value.join(format!("task-completion-{task}.json"));
                    }
                    Some("--admission") => admission_receipt = PathBuf::from(value),
                    Some("--aggregate") => aggregate_receipt = PathBuf::from(value),
                    Some("--verification") => verification_receipt = PathBuf::from(value),
                    Some("--receipt") if command_name == "task-admit" => {
                        admission_receipt = PathBuf::from(value);
                    }
                    Some("--receipt") if command_name == "task-verify" => {
                        gate_receipt = PathBuf::from(value);
                    }
                    Some("--receipt") => completion_receipt = PathBuf::from(value),
                    Some("--closure-receipt") => closure_receipt = Some(PathBuf::from(value)),
                    Some("--revocation") => revocations.push(PathBuf::from(value)),
                    Some("--shard") => shards.push(PathBuf::from(value)),
                    Some("--dependency-receipt") => dependencies.push(PathBuf::from(value)),
                    _ => {
                        return Err(format!(
                            "unsupported task gate flag {}",
                            flag.to_string_lossy()
                        ))
                    }
                }
            }
            Some(other) => return Err(format!("unknown argument: {other}")),
            None => return Err("non-UTF-8 task gate flag".to_owned()),
        }
    }
    if revocations.is_empty() {
        revocations = vec![
            repo_root.join(".omo/evidence/completion-revocation.json"),
            repo_root.join(".omo/evidence/fast-verification-completion-revocation-20260805.json"),
        ];
    }
    Ok(ParsedArgs {
        input: TaskGateInput {
            task,
            candidate_sha256,
            boulder,
            plan,
            evidence_root: evidence_root_value,
            admission_receipt,
            aggregate_receipt,
            verification_receipt,
            gate_receipt,
            completion_receipt,
            revocations,
            shards,
            dependencies,
            finalize_closure,
            closure_receipt,
        },
    })
}

fn current_head(repo_root: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", &repo_root.to_string_lossy(), "rev-parse", "HEAD"])
        .env("GIT_MASTER", "1")
        .output()
        .map_err(|error| format!("git head: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git head: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| format!("git head is not UTF-8: {error}"))
}
