use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness_testkit::tui_fidelity_deadline::{
    CommandSpec, CommandStatus, DeadlineRunner, InterruptFlag, ResourceLimits,
};
use harness_testkit::tui_fidelity_matrix::{
    execute_matrix_bounded, read_coverage_documents, MatrixError, MatrixExecution,
};
use harness_testkit::tui_fidelity_scheduler::BoundedScheduler;

pub(super) fn execute(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse(arguments, repo_root)?;
    let (inventory, manifest, report) =
        read_coverage_documents(&args.inventory, &args.manifest).map_err(matrix_error)?;
    let executable = env::current_exe().map_err(|error| format!("current executable: {error}"))?;
    let reference_bin = args.reference_bin.clone();
    let harness_bin = args.harness_bin.clone();
    let browser_bin = args.browser_bin.clone();
    let font_family = args.font_family.clone();
    let node_modules = args.node_modules.clone();
    let timeout_ms = args.timeout_ms.to_string();
    let workers = BoundedScheduler::with_default_workers().workers();
    let started = Instant::now();
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
                OsString::from("--reference-receipt"),
                repo_root
                    .join(".omo/evidence/task-2-grok-build-tui-experiential-parity/receipt.json")
                    .into_os_string(),
                OsString::from("--reference-root"),
                repo_root.join("inspirations/grok-build").into_os_string(),
                OsString::from("--harness-bin"),
                harness_bin.as_os_str().to_owned(),
                OsString::from("--evidence-dir"),
                execution.evidence_dir.as_os_str().to_owned(),
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
            let remaining = Duration::from_secs(120)
                .checked_sub(started.elapsed())
                .unwrap_or(Duration::from_millis(1));
            let output = DeadlineRunner::new(
                remaining,
                Duration::from_secs(2),
                ResourceLimits::verification_default(),
                interrupt.clone(),
            )
            .run(&command)
            .map_err(|error| MatrixError::Execution(format!("compare process: {error}")))?;
            let detail = format_deadline_output(&output);
            if output.status == CommandStatus::Passed {
                Ok((true, true, detail))
            } else if output.status == CommandStatus::Failed && detail.contains("comparison:") {
                Ok((true, false, detail))
            } else {
                Ok((false, false, detail))
            }
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

struct MatrixArgs {
    inventory: PathBuf,
    manifest: PathBuf,
    evidence_root: PathBuf,
    reference_bin: PathBuf,
    harness_bin: PathBuf,
    browser_bin: Option<PathBuf>,
    font_family: String,
    node_modules: Option<PathBuf>,
    timeout_ms: u64,
}

fn parse(arguments: Vec<OsString>, repo_root: &Path) -> Result<MatrixArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new("matrix")) {
        return Err("usage: matrix --suite complete --reference-bin PATH --harness-bin PATH --evidence-root PATH".to_owned());
    }
    let mut suite = None;
    let mut inventory = repo_root.join("configs/tui-fidelity-requirement-inventory.json");
    let mut manifest = repo_root.join("configs/tui-fidelity-coverage-manifest.json");
    let mut evidence_root = None;
    let mut reference_bin = None;
    let mut harness_bin = None;
    let mut browser_bin = None;
    let mut node_modules = None;
    let mut font_family = "DejaVu Sans Mono".to_owned();
    let mut timeout_ms = 20_000;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--suite") => suite = Some(value.to_string_lossy().into_owned()),
            Some("--inventory") => inventory = PathBuf::from(value),
            Some("--manifest") => manifest = PathBuf::from(value),
            Some("--evidence-root") => evidence_root = Some(PathBuf::from(value)),
            Some("--reference-bin") => reference_bin = Some(PathBuf::from(value)),
            Some("--harness-bin") => harness_bin = Some(PathBuf::from(value)),
            Some("--browser-bin") => browser_bin = Some(PathBuf::from(value)),
            Some("--node-modules") => node_modules = Some(PathBuf::from(value)),
            Some("--font-family") => font_family = value.to_string_lossy().into_owned(),
            Some("--timeout-ms") => {
                timeout_ms = value
                    .to_string_lossy()
                    .parse()
                    .map_err(|error| format!("invalid timeout: {error}"))?
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    if suite.as_deref() != Some("complete") {
        return Err("matrix requires --suite complete".to_owned());
    }
    Ok(MatrixArgs {
        inventory,
        manifest,
        evidence_root: evidence_root.ok_or("missing --evidence-root")?,
        reference_bin: reference_bin.ok_or("missing --reference-bin")?,
        harness_bin: harness_bin.ok_or("missing --harness-bin")?,
        browser_bin,
        font_family,
        node_modules,
        timeout_ms,
    })
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
