use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity_closure::{
    complete_boulder_atomically, verify_closure, ClosureContract, ClosureError,
    ClosureVerificationInput, ReviewReceiptInput,
};

pub(super) fn execute_verify(arguments: Vec<OsString>, repo_root: &Path) -> Result<(), String> {
    let args = parse_verify(arguments, repo_root)?;
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

pub(super) fn execute_complete(arguments: Vec<OsString>) -> Result<(), String> {
    let args = parse_complete(arguments)?;
    let receipt = read(&args.receipt)?;
    complete_boulder_atomically(&args.boulder, &receipt).map_err(closure_error)?;
    println!(
        "tui-fidelity closure-complete PASS: {}",
        args.boulder.display()
    );
    Ok(())
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

fn parse_verify(arguments: Vec<OsString>, repo_root: &Path) -> Result<ClosureVerifyArgs, String> {
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

fn parse_complete(arguments: Vec<OsString>) -> Result<ClosureCompleteArgs, String> {
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

fn closure_error(error: ClosureError) -> String {
    error.to_string()
}
