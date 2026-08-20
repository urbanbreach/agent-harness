use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use harness_testkit::tui_fidelity_deadline::{
    CommandSpec, CommandStatus, DeadlineRunner, InterruptFlag, ResourceLimits,
};
use harness_testkit::tui_fidelity_matrix::{
    execute_matrix_bounded, read_coverage_documents, validate_scenario_registry, MatrixError,
    MatrixExecution,
};
use harness_testkit::tui_fidelity_scheduler::BoundedScheduler;

pub(super) fn execute(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse(arguments, repo_root)?;
    let (inventory, manifest, report) =
        read_coverage_documents(&args.inventory, &args.manifest).map_err(matrix_error)?;
    let registry_json = fs::read_to_string(&args.scenario_registry).map_err(|error| {
        format!(
            "scenario registry {}: {error}",
            args.scenario_registry.display()
        )
    })?;
    validate_scenario_registry(&registry_json, &manifest).map_err(matrix_error)?;
    let executable = env::current_exe().map_err(|error| format!("current executable: {error}"))?;
    let reference_bin = args.reference_bin.clone();
    let reference_authority = args.reference_authority.clone();
    let reference_receipt = args.reference_receipt.clone();
    let reference_root = args.reference_root.clone();
    let harness_bin = args.harness_bin.clone();
    let candidate_receipt = args.candidate_receipt.clone();
    let browser_bin = args.browser_bin.clone();
    let font_family = args.font_family.clone();
    let node_modules = args.node_modules.clone();
    let timeout_ms = args.timeout_ms.to_string();
    let workers = BoundedScheduler::with_default_workers().workers();
    let interrupt = InterruptFlag::install().map_err(|error| error.to_string())?;
    let receipt = execute_matrix_bounded(
        manifest,
        report,
        "complete",
        &args.evidence_root,
        workers,
        |execution: MatrixExecution| {
            let mut command = CommandSpec::new(&executable).args([
                OsString::from("compare"),
                OsString::from("--scenario"),
                OsString::from(&execution.row.scenario_id),
                OsString::from("--reference-bin"),
                reference_bin.as_os_str().to_owned(),
                OsString::from("--reference-authority"),
                reference_authority.as_os_str().to_owned(),
                OsString::from("--reference-receipt"),
                reference_receipt.as_os_str().to_owned(),
                OsString::from("--reference-root"),
                reference_root.as_os_str().to_owned(),
                OsString::from("--harness-bin"),
                harness_bin.as_os_str().to_owned(),
                OsString::from("--candidate-receipt"),
                candidate_receipt.as_os_str().to_owned(),
                OsString::from("--evidence-dir"),
                execution.evidence_dir.as_os_str().to_owned(),
                OsString::from("--acceptance"),
                OsString::from("full-parity"),
                OsString::from("--coverage-row-id"),
                OsString::from(&execution.row.row_id),
                OsString::from("--coverage-action-path"),
                OsString::from(&execution.row.action_path),
                OsString::from("--coverage-viewport"),
                OsString::from(format!(
                    "{}x{}",
                    execution.row.viewport.cols, execution.row.viewport.rows
                )),
                OsString::from("--coverage-terminal-tier"),
                OsString::from(&execution.row.terminal_tier),
                OsString::from("--coverage-persona"),
                OsString::from(&execution.row.persona),
                OsString::from("--coverage-theme-mode"),
                OsString::from(&execution.row.theme_mode),
                OsString::from("--coverage-media-mode"),
                OsString::from(&execution.row.media_mode),
                OsString::from("--coverage-failure-path"),
                OsString::from(&execution.row.failure_path),
                OsString::from("--coverage-trial"),
                OsString::from(execution.trial.to_string()),
                OsString::from("--font-family"),
                OsString::from(&font_family),
                OsString::from("--timeout-ms"),
                OsString::from(&timeout_ms),
            ]);
            if let Some(browser_bin) = &browser_bin {
                command = command.args([
                    OsString::from("--browser-bin"),
                    browser_bin.as_os_str().to_owned(),
                ]);
            }
            if let Some(node_modules) = &node_modules {
                command = command.args([
                    OsString::from("--node-modules"),
                    node_modules.as_os_str().to_owned(),
                ]);
            }
            let output = DeadlineRunner::new(
                Duration::from_millis(args.trial_deadline_ms),
                Duration::from_secs(2),
                ResourceLimits::verification_default(),
                interrupt.clone(),
            )
            .run(&command)
            .map_err(|error| MatrixError::Execution(format!("compare process: {error}")))?;
            let detail = format_deadline_output(&output);
            let (captured, compared) = comparison_outcome(&execution.evidence_dir)
                .unwrap_or((output.status == CommandStatus::Passed, false));
            Ok((captured, compared, detail))
        },
    )
    .map_err(matrix_error)?;
    println!(
        "tui-fidelity matrix PASS: {} requirements, {} rows, {} capture keys, {} workers, evidence {}",
        inventory.requirements.len(),
        receipt.report.row_count,
        receipt.report.capture_key_count,
        workers,
        args.evidence_root.display()
    );
    Ok(())
}

#[derive(Debug)]
struct MatrixArgs {
    inventory: PathBuf,
    manifest: PathBuf,
    scenario_registry: PathBuf,
    evidence_root: PathBuf,
    reference_bin: PathBuf,
    reference_authority: PathBuf,
    reference_receipt: PathBuf,
    reference_root: PathBuf,
    harness_bin: PathBuf,
    candidate_receipt: PathBuf,
    browser_bin: Option<PathBuf>,
    font_family: String,
    node_modules: Option<PathBuf>,
    timeout_ms: u64,
    trial_deadline_ms: u64,
}

fn parse(arguments: Vec<OsString>, repo_root: &Path) -> Result<MatrixArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new("matrix")) {
        return Err("usage: matrix --suite complete --reference-authority PATH --reference-bin PATH --reference-root PATH --reference-receipt PATH --harness-bin PATH --candidate-receipt PATH --evidence-root PATH".to_owned());
    }
    let mut suite = None;
    let mut inventory = repo_root.join("configs/tui-fidelity-requirement-inventory.json");
    let mut manifest = repo_root.join("configs/tui-fidelity-coverage-manifest.json");
    let mut scenario_registry =
        repo_root.join("crates/harness-testkit/src/tui_fidelity_scenarios/baseline/registry.json");
    let mut evidence_root = None;
    let mut reference_bin = None;
    let mut reference_authority = None;
    let mut reference_receipt = None;
    let mut reference_root = None;
    let mut harness_bin = None;
    let mut candidate_receipt = None;
    let mut browser_bin = None;
    let mut node_modules = None;
    let mut font_family = "DejaVu Sans Mono".to_owned();
    let mut timeout_ms = 20_000;
    let mut trial_deadline_ms = 120_000;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--suite") => suite = Some(value.to_string_lossy().into_owned()),
            Some("--inventory") => inventory = PathBuf::from(value),
            Some("--manifest") => manifest = PathBuf::from(value),
            Some("--scenario-registry") => scenario_registry = PathBuf::from(value),
            Some("--evidence-root") => evidence_root = Some(PathBuf::from(value)),
            Some("--reference-bin") => reference_bin = Some(PathBuf::from(value)),
            Some("--reference-authority") => reference_authority = Some(PathBuf::from(value)),
            Some("--reference-receipt") => reference_receipt = Some(PathBuf::from(value)),
            Some("--reference-root") => reference_root = Some(PathBuf::from(value)),
            Some("--harness-bin") => harness_bin = Some(PathBuf::from(value)),
            Some("--candidate-receipt") => candidate_receipt = Some(PathBuf::from(value)),
            Some("--browser-bin") => browser_bin = Some(PathBuf::from(value)),
            Some("--node-modules") => node_modules = Some(PathBuf::from(value)),
            Some("--font-family") => font_family = value.to_string_lossy().into_owned(),
            Some("--timeout-ms") => {
                timeout_ms = value
                    .to_string_lossy()
                    .parse()
                    .map_err(|error| format!("invalid timeout: {error}"))?
            }
            Some("--trial-deadline-ms") => {
                trial_deadline_ms = value
                    .to_string_lossy()
                    .parse()
                    .map_err(|error| format!("invalid trial deadline: {error}"))?
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    if suite.as_deref() != Some("complete") {
        return Err("matrix requires --suite complete".to_owned());
    }
    let evidence_root = evidence_root.ok_or("missing --evidence-root")?;
    if evidence_root.exists() {
        return Err("--evidence-root must be a fresh path".to_owned());
    }
    if timeout_ms == 0 || trial_deadline_ms <= timeout_ms {
        return Err("trial deadline must be greater than the nonzero scenario timeout".to_owned());
    }
    Ok(MatrixArgs {
        inventory,
        manifest,
        scenario_registry,
        evidence_root,
        reference_bin: reference_bin.ok_or("missing --reference-bin")?,
        reference_authority: reference_authority.ok_or("missing --reference-authority")?,
        reference_receipt: reference_receipt.ok_or("missing --reference-receipt")?,
        reference_root: reference_root.ok_or("missing --reference-root")?,
        harness_bin: harness_bin.ok_or("missing --harness-bin")?,
        candidate_receipt: candidate_receipt.ok_or("missing --candidate-receipt")?,
        browser_bin,
        font_family,
        node_modules,
        timeout_ms,
        trial_deadline_ms,
    })
}

fn comparison_outcome(evidence_dir: &Path) -> Option<(bool, bool)> {
    let bytes = fs::read(evidence_dir.join("comparison.json")).ok()?;
    let receipt: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some((
        receipt.get("capture_succeeded")?.as_bool()?,
        receipt.get("comparison_passed")?.as_bool()?,
    ))
}

fn format_deadline_output(
    output: &harness_testkit::tui_fidelity_deadline::CommandReceipt,
) -> String {
    format!(
        "stdout: {}; stderr: {}; status: {:?}; duration_ms: {}",
        output.stdout, output.stderr, output.status, output.duration_millis
    )
}

fn matrix_error(error: MatrixError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_requires_complete_provenance_and_independent_trial_deadline() {
        // arrange
        let temp = tempfile::tempdir().expect("temporary root");
        let evidence = temp.path().join("fresh-evidence");
        let arguments = arguments(&evidence);

        // act
        let parsed = parse(arguments, Path::new("/repo")).expect("complete matrix arguments");

        // assert
        assert_eq!(parsed.reference_authority, PathBuf::from("authority"));
        assert_eq!(parsed.reference_receipt, PathBuf::from("reference-receipt"));
        assert_eq!(parsed.reference_root, PathBuf::from("reference-root"));
        assert_eq!(parsed.candidate_receipt, PathBuf::from("candidate-receipt"));
        assert_eq!(parsed.timeout_ms, 20_000);
        assert_eq!(parsed.trial_deadline_ms, 45_000);
    }

    #[test]
    fn parser_rejects_missing_candidate_receipt_and_reused_evidence_root() {
        // arrange
        let temp = tempfile::tempdir().expect("temporary root");
        let fresh = temp.path().join("fresh-evidence");
        let mut missing_receipt = arguments(&fresh);
        let position = missing_receipt
            .iter()
            .position(|value| value == "--candidate-receipt")
            .expect("candidate receipt flag");
        missing_receipt.drain(position..=position + 1);

        // act
        let missing = parse(missing_receipt, Path::new("/repo"));
        let reused = parse(arguments(temp.path()), Path::new("/repo"));

        // assert
        assert!(missing
            .expect_err("candidate receipt is mandatory")
            .contains("missing --candidate-receipt"));
        assert!(reused
            .expect_err("evidence root must be fresh")
            .contains("fresh path"));
    }

    fn arguments(evidence: &Path) -> Vec<OsString> {
        [
            "matrix",
            "--suite",
            "complete",
            "--reference-bin",
            "reference",
            "--reference-authority",
            "authority",
            "--reference-receipt",
            "reference-receipt",
            "--reference-root",
            "reference-root",
            "--harness-bin",
            "harness",
            "--candidate-receipt",
            "candidate-receipt",
            "--evidence-root",
            evidence.to_str().expect("UTF-8 evidence path"),
            "--trial-deadline-ms",
            "45000",
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }
}
