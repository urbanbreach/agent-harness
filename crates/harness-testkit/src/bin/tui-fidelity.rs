use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

#[path = "../tui_fidelity_baseline.rs"]
mod tui_fidelity_baseline;

#[path = "tui_fidelity_commands/mod.rs"]
mod tui_fidelity_commands;

use harness_testkit::binary_receipt::read_receipt;
use harness_testkit::tui_fidelity::{AdapterKind, Scenario};
use harness_testkit::tui_fidelity_cache::ReferenceCache;
use harness_testkit::tui_fidelity_compare::AcceptanceProfile;
use harness_testkit::tui_fidelity_runner::{
    record_preflight_failure, run_compare_with_cached_reference_and_profile, CandidateBinding,
    RendererConfig, RunnerConfig, RunnerError, RunnerTiming, RuntimeBinary, SourceGuardConfig,
};

const STARTUP_SMOKE: &str = include_str!("../../tests/fixtures/tui_fidelity/startup-smoke.json");
const PACKET2_SUSTAINED_STREAM: &str =
    include_str!("../../tests/fixtures/tui_fidelity/packet2-sustained-stream.json");
const REFERENCE_REVISION: &str = "be713136d2a69080743a3f6b3c72077057e5948f";

struct CompareArgs {
    scenario: String,
    reference_bin: PathBuf,
    reference_receipt: PathBuf,
    reference_root: PathBuf,
    harness_bin: PathBuf,
    candidate_receipt: PathBuf,
    evidence_dir: PathBuf,
    browser_bin: Option<PathBuf>,
    font_family: String,
    node_modules: Option<PathBuf>,
    timeout: Duration,
    acceptance_profile: AcceptanceProfile,
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

fn execute(arguments: Vec<OsString>) -> Result<(), RunnerError> {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if arguments.first().and_then(|value| value.to_str()) != Some("compare") {
        return tui_fidelity_commands::execute(arguments, &repo_root)
            .map_err(|detail| RunnerError::Arguments { detail });
    }
    let args = parse_compare(arguments).map_err(|detail| RunnerError::Arguments { detail })?;
    let (scenario, reference, harness, candidate_binding) = prepare_compare(&args, &repo_root)
        .map_err(|error| record_cli_preflight_failure(&args.evidence_dir, error))?;
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
        candidate_binding,
        source_guard: SourceGuardConfig {
            program: repo_root.join("scripts/tui-fidelity/source-guard.sh"),
            reference_root: args.reference_root,
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
    let cache_root = env::var_os("TUI_FIDELITY_REFERENCE_CACHE").map(PathBuf::from);
    let cache_key = env::var("TUI_FIDELITY_REFERENCE_CACHE_KEY").ok();
    let cache = match (cache_root, cache_key) {
        (Some(root), Some(key)) => Some((ReferenceCache::new(root), key)),
        (None, None) => None,
        (Some(_), None) | (None, Some(_)) => {
            return Err(RunnerError::Arguments {
                detail: "reference cache root and key must be provided together".to_owned(),
            });
        }
    };
    let cached = cache
        .as_ref()
        .map(|(cache, key)| cache.load_reference(key))
        .transpose()
        .map_err(|error| RunnerError::Arguments {
            detail: error.to_string(),
        })?
        .flatten();
    let cache_hit = cached.is_some();
    let receipt = run_compare_with_cached_reference_and_profile(
        &scenario,
        &config,
        cached,
        args.acceptance_profile,
    )?;
    if !cache_hit {
        if let Some((cache, key)) = &cache {
            let reference = receipt
                .runtimes
                .iter()
                .find(|runtime| runtime.adapter == AdapterKind::Grok)
                .ok_or_else(|| RunnerError::Arguments {
                    detail: "comparison receipt has no Grok runtime".to_owned(),
                })?;
            cache
                .publish_reference(key, reference)
                .map_err(|error| RunnerError::Arguments {
                    detail: error.to_string(),
                })?;
        }
    }
    println!(
        "tui-fidelity compare PASS: {} runtimes, evidence {}",
        receipt.runtimes.len(),
        config.evidence_dir.display()
    );
    Ok(())
}

fn prepare_compare(
    args: &CompareArgs,
    repo_root: &Path,
) -> Result<(Scenario, RuntimeBinary, RuntimeBinary, CandidateBinding), RunnerError> {
    let scenario = match args.scenario.as_str() {
        "startup-smoke" => Scenario::from_json(STARTUP_SMOKE).map_err(RunnerError::from)?,
        "packet2-sustained-stream" => {
            Scenario::from_json(PACKET2_SUSTAINED_STREAM).map_err(RunnerError::from)?
        }
        other => tui_fidelity_baseline::load(other, repo_root)?,
    };
    let receipt_path = absolute_path(repo_root, &args.reference_receipt);
    let receipt = read_receipt(&receipt_path).map_err(|error| RunnerError::BinaryReceipt {
        path: receipt_path.clone(),
        detail: error.to_string(),
    })?;
    let reference = checked_binary(ExpectedBinary {
        adapter: AdapterKind::Grok,
        path: &args.reference_bin,
        revision: &receipt.reference.source_revision,
        sha256: &receipt.reference.sha256,
    })?;
    let candidate_sha = current_revision(repo_root)?;
    let candidate_receipt_path = absolute_path(repo_root, &args.candidate_receipt);
    let candidate_binding = read_candidate_binding(&candidate_receipt_path)?;
    if candidate_binding.candidate_sha != candidate_sha {
        return Err(RunnerError::CandidateBinding {
            path: args.harness_bin.clone(),
            detail: format!(
                "candidate receipt SHA {} does not match current Git HEAD {}",
                candidate_binding.candidate_sha, candidate_sha
            ),
        });
    }
    let harness_path = absolute_path(repo_root, &args.harness_bin);
    let harness = checked_current_binary(&harness_path, &candidate_sha)?;
    let runner_path = env::current_exe().map_err(|error| RunnerError::Io {
        path: PathBuf::from("<current-executable>"),
        detail: error.to_string(),
    })?;
    let runner = RuntimeBinary::from_path(&runner_path, &candidate_sha)?;
    let target_dir = harness_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| RunnerError::CandidateBinding {
            path: harness_path.clone(),
            detail: "candidate path is not target/<profile>/debug/harness".to_owned(),
        })?;
    let target_dir =
        fs::canonicalize(target_dir).map_err(|error| RunnerError::CandidateBinding {
            path: harness_path.clone(),
            detail: format!("cannot resolve candidate target directory: {error}"),
        })?;
    if candidate_binding.candidate_binary_sha256 != harness.sha256
        || candidate_binding.runner_sha256 != runner.sha256
        || candidate_binding.target_dir != target_dir
    {
        return Err(RunnerError::CandidateBinding {
            path: harness_path,
            detail: "candidate receipt does not match fresh binary, runner, or target directory"
                .to_owned(),
        });
    }
    Ok((scenario, reference, harness, candidate_binding))
}

fn parse_compare(arguments: Vec<OsString>) -> Result<CompareArgs, String> {
    let mut values = arguments.into_iter();
    if values.next().as_deref() != Some(std::ffi::OsStr::new("compare")) {
        return Err("usage: tui-fidelity compare --scenario ID --reference-bin PATH --reference-receipt PATH --reference-root PATH --harness-bin PATH --candidate-receipt PATH --evidence-dir PATH [--acceptance full-parity|packet2-scheduling] [--browser-bin PATH] [--font-family NAME] [--node-modules PATH] [--timeout-ms N]".to_owned());
    }
    let mut scenario = None;
    let mut reference_bin = None;
    let mut reference_receipt = None;
    let mut reference_root = None;
    let mut harness_bin = None;
    let mut candidate_receipt = None;
    let mut evidence_dir = None;
    let mut browser_bin = None;
    let mut font_family = "DejaVu Sans Mono".to_owned();
    let mut node_modules = None;
    let mut timeout = Duration::from_secs(20);
    let mut acceptance_profile = AcceptanceProfile::FullParity;
    while let Some(flag) = values.next() {
        let value = values
            .next()
            .ok_or_else(|| format!("missing value for {}", flag.to_string_lossy()))?;
        match flag.to_str() {
            Some("--scenario") => scenario = Some(value.to_string_lossy().into_owned()),
            Some("--reference-bin") => reference_bin = Some(PathBuf::from(value)),
            Some("--reference-receipt") => reference_receipt = Some(PathBuf::from(value)),
            Some("--reference-root") => reference_root = Some(PathBuf::from(value)),
            Some("--harness-bin") => harness_bin = Some(PathBuf::from(value)),
            Some("--candidate-receipt") => candidate_receipt = Some(PathBuf::from(value)),
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
            Some("--acceptance") => {
                acceptance_profile = match value.to_str() {
                    Some("full-parity") => AcceptanceProfile::FullParity,
                    Some("packet2-scheduling") => AcceptanceProfile::Packet2Scheduling,
                    _ => return Err("invalid --acceptance profile".to_owned()),
                };
            }
            _ => return Err(format!("unknown argument: {}", flag.to_string_lossy())),
        }
    }
    Ok(CompareArgs {
        scenario: scenario.ok_or("missing --scenario")?,
        reference_bin: reference_bin.ok_or("missing --reference-bin")?,
        reference_receipt: reference_receipt.ok_or("missing --reference-receipt")?,
        reference_root: reference_root.ok_or("missing --reference-root")?,
        harness_bin: harness_bin.ok_or("missing --harness-bin")?,
        candidate_receipt: candidate_receipt.ok_or("missing --candidate-receipt")?,
        evidence_dir: evidence_dir.ok_or("missing --evidence-dir")?,
        browser_bin,
        font_family,
        node_modules,
        timeout,
        acceptance_profile,
    })
}

fn read_candidate_binding(path: &Path) -> Result<CandidateBinding, RunnerError> {
    let bytes = fs::read(path).map_err(|error| RunnerError::BinaryReceipt {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| RunnerError::BinaryReceipt {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

struct ExpectedBinary<'a> {
    adapter: AdapterKind,
    path: &'a Path,
    revision: &'a str,
    sha256: &'a str,
}

fn checked_binary(expected: ExpectedBinary<'_>) -> Result<RuntimeBinary, RunnerError> {
    if !is_executable(expected.path) {
        return Err(RunnerError::MissingBinary {
            adapter: expected.adapter,
            path: expected.path.to_path_buf(),
        });
    }
    let identity = RuntimeBinary::from_path(expected.path, expected.revision)?;
    if identity.sha256 == expected.sha256 {
        Ok(identity)
    } else {
        Err(RunnerError::BinaryDigest {
            path: expected.path.to_path_buf(),
            expected: expected.sha256.to_owned(),
            actual: identity.sha256,
        })
    }
}

fn checked_current_binary(path: &Path, revision: &str) -> Result<RuntimeBinary, RunnerError> {
    if !is_executable(path) {
        return Err(RunnerError::MissingBinary {
            adapter: AdapterKind::Harness,
            path: path.to_path_buf(),
        });
    }
    RuntimeBinary::from_path(path, revision)
}

fn current_revision(repo_root: &Path) -> Result<String, RunnerError> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|error| RunnerError::Arguments {
            detail: format!("read candidate Git revision: {error}"),
        })?;
    if !output.status.success() {
        return Err(RunnerError::Arguments {
            detail: format!(
                "candidate Git revision failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn absolute_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn record_cli_preflight_failure(evidence_dir: &Path, primary: RunnerError) -> RunnerError {
    match record_preflight_failure(evidence_dir, &primary) {
        Ok(()) => primary,
        Err(cleanup) => RunnerError::Cleanup {
            primary: Some(Box::new(primary)),
            detail: format!("cleanup receipt: {cleanup}"),
        },
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
