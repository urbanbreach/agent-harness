use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use harness_testkit::binary_receipt::read_receipt;
use harness_testkit::tui_fidelity::Scenario;
use harness_testkit::tui_fidelity_runner::{
    record_preflight_failure, run_compare, RendererConfig, RunnerConfig, RunnerError, RunnerTiming,
    RuntimeBinary, SourceGuardConfig,
};

const STARTUP_SMOKE: &str = include_str!("../../tests/fixtures/tui_fidelity/startup-smoke.json");
const REFERENCE_REVISION: &str = "500129c714ad1b10e6095481f4a8387a2ec52649";

struct CompareArgs {
    scenario: String,
    reference_bin: PathBuf,
    harness_bin: PathBuf,
    evidence_dir: PathBuf,
    browser_bin: Option<PathBuf>,
    font_family: String,
    node_modules: Option<PathBuf>,
    timeout: Duration,
}

fn main() -> ExitCode {
    match execute(env::args_os().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tui-fidelity: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(arguments: Vec<OsString>) -> Result<(), String> {
    let args = parse_compare(arguments)?;
    let scenario = match args.scenario.as_str() {
        "startup-smoke" => Scenario::from_json(STARTUP_SMOKE),
        other => {
            let error = RunnerError::UnknownScenario {
                id: other.to_owned(),
            };
            record_preflight_failure(&args.evidence_dir, &error)
                .map_err(|cleanup_error| cleanup_error.to_string())?;
            return Err(error.to_string());
        }
    }
    .map_err(|error| error.to_string())?;
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let receipt_path =
        repo_root.join(".omo/evidence/task-2-grok-build-tui-experiential-parity/receipt.json");
    let receipt = read_receipt(&receipt_path).map_err(|error| error.to_string())?;
    receipt
        .verify_binary_digests()
        .map_err(|error| error.to_string())?;
    let reference = checked_binary(
        &args.reference_bin,
        &receipt.reference.source_revision,
        &receipt.reference.sha256,
    )?;
    let harness = checked_binary(
        &args.harness_bin,
        &receipt.harness.source_revision,
        &receipt.harness.sha256,
    )?;
    let browser_program = args
        .browser_bin
        .or_else(discover_browser)
        .unwrap_or_else(|| PathBuf::from("missing-browser"));
    let node_modules = args.node_modules.or_else(|| {
        let candidate = repo_root
            .join(".omo/evidence/task-4-grok-build-tui-experiential-parity/tooling/node_modules");
        candidate.is_dir().then_some(candidate)
    });
    let config = RunnerConfig {
        repo_root: repo_root.clone(),
        evidence_dir: args.evidence_dir,
        reference,
        harness,
        source_guard: SourceGuardConfig {
            program: repo_root.join("scripts/tui-fidelity/source-guard.sh"),
            reference_root: PathBuf::from("inspirations/grok-build"),
            revision: REFERENCE_REVISION.to_owned(),
        },
        renderer: RendererConfig {
            node_program: PathBuf::from("node"),
            script: repo_root.join("scripts/tui-parity/web-terminal-visual-qa.mjs"),
            browser_program,
            font_family: args.font_family,
            node_modules,
        },
        timing: RunnerTiming {
            tick: Duration::from_millis(75),
            scenario_timeout: args.timeout,
            normal_exit_timeout: Duration::from_secs(5),
            cleanup_timeout: Duration::from_secs(2),
        },
    };
    let receipt = run_compare(&scenario, &config).map_err(|error| error.to_string())?;
    println!(
        "tui-fidelity compare PASS: {} runtimes, evidence {}",
        receipt.runtimes.len(),
        config.evidence_dir.display()
    );
    Ok(())
}

fn parse_compare(arguments: Vec<OsString>) -> Result<CompareArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(std::ffi::OsStr::new("compare")) {
        return Err("usage: tui-fidelity compare --scenario ID --reference-bin PATH --harness-bin PATH --evidence-dir PATH [--browser-bin PATH] [--font-family NAME] [--node-modules PATH] [--timeout-ms N]".to_owned());
    }
    let mut scenario = None;
    let mut reference_bin = None;
    let mut harness_bin = None;
    let mut evidence_dir = None;
    let mut browser_bin = None;
    let mut font_family = "DejaVu Sans Mono".to_owned();
    let mut node_modules = None;
    let mut timeout = Duration::from_secs(20);
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--scenario") => scenario = Some(value.to_string_lossy().into_owned()),
            Some("--reference-bin") => reference_bin = Some(PathBuf::from(value)),
            Some("--harness-bin") => harness_bin = Some(PathBuf::from(value)),
            Some("--evidence-dir") => evidence_dir = Some(PathBuf::from(value)),
            Some("--browser-bin") => browser_bin = Some(PathBuf::from(value)),
            Some("--font-family") => font_family = value.to_string_lossy().into_owned(),
            Some("--node-modules") => node_modules = Some(PathBuf::from(value)),
            Some("--timeout-ms") => {
                let millis = value
                    .to_string_lossy()
                    .parse::<u64>()
                    .map_err(|error| format!("invalid --timeout-ms: {error}"))?;
                timeout = Duration::from_millis(millis);
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    Ok(CompareArgs {
        scenario: scenario.ok_or("missing --scenario")?,
        reference_bin: reference_bin.ok_or("missing --reference-bin")?,
        harness_bin: harness_bin.ok_or("missing --harness-bin")?,
        evidence_dir: evidence_dir.ok_or("missing --evidence-dir")?,
        browser_bin,
        font_family,
        node_modules,
        timeout,
    })
}

fn checked_binary(path: &Path, revision: &str, expected: &str) -> Result<RuntimeBinary, String> {
    let identity = RuntimeBinary::from_path(path, revision).map_err(|error| error.to_string())?;
    if identity.sha256 == expected {
        Ok(identity)
    } else {
        Err(format!(
            "binary digest mismatch for {}: expected {expected}, got {}",
            path.display(),
            identity.sha256
        ))
    }
}

fn discover_browser() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CHROME_BIN").map(PathBuf::from) {
        if is_executable(&path) {
            return Some(path);
        }
    }
    for path in ["/usr/bin/google-chrome", "/usr/bin/chromium"] {
        let candidate = PathBuf::from(path);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    let cache = env::var_os("HOME")
        .map(PathBuf::from)?
        .join(".cache/ms-playwright");
    let mut candidates = fs::read_dir(cache)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("chrome-linux64/chrome"))
        .filter(|path| is_executable(path))
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
