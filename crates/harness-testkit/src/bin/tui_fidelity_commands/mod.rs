use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness_testkit::tui_fidelity_closure::{
    complete_boulder_atomically, verify_closure, ClosureContract, ClosureError,
    ClosureVerificationInput, ReviewReceiptInput,
};
use harness_testkit::tui_fidelity_deadline::{
    CommandSpec, CommandStatus, DeadlineRunner, InterruptFlag, ResourceLimits,
};
use harness_testkit::tui_fidelity_matrix::{
    execute_matrix, read_coverage_documents, MatrixError, MatrixTrial,
};

mod verify;
mod verify_executor;

pub fn execute(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    match arguments.first().and_then(|value| value.to_str()) {
        Some("matrix") => execute_matrix_command(arguments, repo_root),
        Some("verify") => verify::execute(arguments, repo_root),
        Some("closure-verify") => execute_closure_verify(arguments, repo_root),
        Some("closure-complete") => execute_closure_complete(arguments),
        Some(command) => Err(format!("unknown tui-fidelity command {command}")),
        None => Err("missing tui-fidelity command".to_owned()),
    }
}

fn execute_matrix_command(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse_matrix(arguments, repo_root)?;
    let (inventory, manifest, report) =
        read_coverage_documents(&args.inventory, &args.manifest).map_err(matrix_error)?;
    let executable = env::current_exe().map_err(|error| format!("current executable: {error}"))?;
    let reference_bin = args.reference_bin.clone();
    let harness_bin = args.harness_bin.clone();
    let browser_bin = args.browser_bin.clone();
    let font_family = args.font_family.clone();
    let node_modules = args.node_modules.clone();
    let timeout_ms = args.timeout_ms.to_string();
    let started = Instant::now();
    let interrupt = InterruptFlag::install().map_err(|error| error.to_string())?;
    let receipt = execute_matrix(
        manifest,
        report,
        "complete",
        &args.evidence_root,
        |trial: MatrixTrial| {
            let mut command = CommandSpec::new(&executable).args([
                OsString::from("compare"),
                OsString::from("--scenario"),
                OsString::from(&trial.row.scenario_id),
                OsString::from("--reference-bin"),
                reference_bin.as_os_str().to_owned(),
                OsString::from("--harness-bin"),
                harness_bin.as_os_str().to_owned(),
                OsString::from("--evidence-dir"),
                trial.evidence_dir.as_os_str().to_owned(),
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
        "tui-fidelity matrix PASS: {} requirements, {} rows, {} trials, evidence {}",
        inventory.requirements.len(),
        receipt.report.row_count,
        receipt.report.trial_count,
        args.evidence_root.display()
    );
    Ok(())
}

fn execute_closure_verify(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse_closure_verify(arguments, repo_root)?;
    let contract_json = read(&args.contract)?;
    let contract: ClosureContract = serde_json::from_str(&contract_json)
        .map_err(|error| format!("closure contract: {error}"))?;
    let reviews = contract
        .preliminary_reviews
        .iter()
        .map(|reference| {
            let path = resolve(repo_root, &reference.receipt_path);
            read(&path).map(|json| ReviewReceiptInput {
                review_id: reference.review_id.clone(),
                json,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let plan = read_bytes(&args.plan)?;
    let inventory = read(&args.inventory)?;
    let manifest = read(&args.manifest)?;
    let revocation = read(&args.revocation)?;
    let receipt = verify_closure(ClosureVerificationInput {
        contract_json: &contract_json,
        plan_bytes: &plan,
        inventory_json: &inventory,
        manifest_json: &manifest,
        revocation_json: &revocation,
        review_receipts: &reviews,
        candidate_sha256: &args.candidate_sha256,
        evidence_root: &args.evidence_root.to_string_lossy(),
    })
    .map_err(closure_error)?;
    fs::create_dir_all(&args.evidence_root).map_err(|error| format!("evidence root: {error}"))?;
    let receipt_path = args.evidence_root.join("closure-receipt.json");
    fs::write(&receipt_path, receipt.json).map_err(|error| format!("closure receipt: {error}"))?;
    println!(
        "tui-fidelity closure-verify PASS: {}",
        receipt_path.display()
    );
    Ok(())
}

fn execute_closure_complete(arguments: Vec<OsString>) -> Result<(), String> {
    let args = parse_closure_complete(arguments)?;
    let receipt = read(&args.receipt)?;
    complete_boulder_atomically(&args.boulder, &receipt).map_err(closure_error)?;
    println!(
        "tui-fidelity closure-complete PASS: {}",
        args.boulder.display()
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

struct ClosureVerifyArgs {
    contract: PathBuf,
    plan: PathBuf,
    inventory: PathBuf,
    manifest: PathBuf,
    revocation: PathBuf,
    evidence_root: PathBuf,
    candidate_sha256: String,
}

struct ClosureCompleteArgs {
    boulder: PathBuf,
    receipt: PathBuf,
}

fn parse_matrix(arguments: Vec<OsString>, repo_root: &Path) -> Result<MatrixArgs, String> {
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

fn parse_closure_verify(
    arguments: Vec<OsString>,
    repo_root: &Path,
) -> Result<ClosureVerifyArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new("closure-verify")) {
        return Err("usage: closure-verify --candidate-sha PATH --evidence-root PATH".to_owned());
    }
    let mut args = ClosureVerifyArgs {
        contract: repo_root.join("configs/tui-fidelity-closure-contract.json"),
        plan: repo_root.join(".omo/plans/grok-build-tui-experiential-parity.md"),
        inventory: repo_root.join("configs/tui-fidelity-requirement-inventory.json"),
        manifest: repo_root.join("configs/tui-fidelity-coverage-manifest.json"),
        revocation: repo_root.join(".omo/evidence/completion-revocation.json"),
        evidence_root: PathBuf::new(),
        candidate_sha256: String::new(),
    };
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--contract") => args.contract = PathBuf::from(value),
            Some("--plan") => args.plan = PathBuf::from(value),
            Some("--inventory") => args.inventory = PathBuf::from(value),
            Some("--manifest") => args.manifest = PathBuf::from(value),
            Some("--revocation") => args.revocation = PathBuf::from(value),
            Some("--evidence-root") => args.evidence_root = PathBuf::from(value),
            Some("--candidate-sha") => args.candidate_sha256 = value.to_string_lossy().into_owned(),
            Some("--phase") => {}
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    if args.evidence_root.as_os_str().is_empty() || args.candidate_sha256.is_empty() {
        return Err("closure-verify requires --evidence-root and --candidate-sha".to_owned());
    }
    Ok(args)
}

fn parse_closure_complete(arguments: Vec<OsString>) -> Result<ClosureCompleteArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(OsStr::new("closure-complete")) {
        return Err("usage: closure-complete --boulder PATH --receipt PATH".to_owned());
    }
    let mut boulder = None;
    let mut receipt = None;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--boulder") => boulder = Some(PathBuf::from(value)),
            Some("--receipt") => receipt = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    Ok(ClosureCompleteArgs {
        boulder: boulder.ok_or("missing --boulder")?,
        receipt: receipt.ok_or("missing --receipt")?,
    })
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn resolve(base: &Path, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        candidate
    } else {
        base.join(candidate)
    }
}

fn format_command_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "stdout: {stdout}; stderr: {stderr}; status: {}",
        output.status
    )
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

fn closure_error(error: ClosureError) -> String {
    error.to_string()
}
