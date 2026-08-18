use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use harness_testkit::tui_fidelity_cache::ReferenceCacheInputs;
use harness_testkit::tui_fidelity_compare::{hash_bytes, COMPARISON_RECEIPT_SCHEMA};
use harness_testkit::tui_fidelity_deadline::{
    CommandSpec, CommandStatus, DeadlineRunner, InterruptFlag, ResourceLimits,
};
use harness_testkit::tui_fidelity_obligation::{CaptureKey, VerificationKey};
use harness_testkit::tui_fidelity_staging::JobIsolation;
use harness_testkit::tui_fidelity_verify::VerificationProfile;

use super::verify::VerifyArgs;

const REFERENCE_REVISION: &str = "eb267feff13129e568df38fb6fdf0ceb65f735d6";

pub(super) struct VerifyExecutor {
    repo_root: PathBuf,
    executable: PathBuf,
    reference_bin: PathBuf,
    harness_bin: PathBuf,
    candidate_receipt: PathBuf,
    browser_bin: Option<PathBuf>,
    node_modules: Option<PathBuf>,
    font_family: String,
    cache_root: PathBuf,
    profile: VerificationProfile,
    started: Instant,
    interrupt: InterruptFlag,
    reference_digest: String,
    source_digest: String,
    browser_version: String,
    xterm_version: String,
    node_pty_version: String,
}

impl VerifyExecutor {
    pub(super) fn new(
        repo_root: &Path,
        args: &VerifyArgs,
        interrupt: InterruptFlag,
    ) -> Result<Self, String> {
        let reference_digest = hash_file(&args.reference_bin)?;
        let source_digest =
            hash_bytes(REFERENCE_REVISION.as_bytes()).map_err(|error| error.to_string())?;
        let browser_version = args.browser_bin.as_ref().map_or_else(
            || "auto-discovered".to_owned(),
            |path| path.display().to_string(),
        );
        let xterm_version = package_version(args.node_modules.as_deref(), "@xterm/xterm")?;
        let node_pty_version = package_version(args.node_modules.as_deref(), "node-pty")?;
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            executable: std::env::current_exe().map_err(|error| error.to_string())?,
            reference_bin: args.reference_bin.clone(),
            harness_bin: args.harness_bin.clone(),
            candidate_receipt: args.candidate_receipt.clone(),
            browser_bin: args.browser_bin.clone(),
            node_modules: args.node_modules.clone(),
            font_family: args.font_family.clone(),
            cache_root: args.evidence_root.join("reference-cache"),
            profile: args.profile,
            started: Instant::now(),
            interrupt,
            reference_digest,
            source_digest,
            browser_version,
            xterm_version,
            node_pty_version,
        })
    }

    pub(super) fn execute(
        &self,
        key: &VerificationKey,
        isolation: &JobIsolation,
    ) -> Result<PathBuf, String> {
        match key {
            VerificationKey::DualCapture(capture) => self.capture(capture, isolation),
            VerificationKey::OwnerTest { key } => self.owner_test(key, isolation),
            VerificationKey::StaticGate { key } => write_receipt(isolation, key, "static_gate"),
            VerificationKey::ReviewerReceipt { key } => self.reviewer(key, isolation),
        }
    }

    fn capture(&self, key: &CaptureKey, isolation: &JobIsolation) -> Result<PathBuf, String> {
        let canonical = key.canonical_json().map_err(|error| error.to_string())?;
        let scenario_digest =
            hash_bytes(canonical.as_bytes()).map_err(|error| error.to_string())?;
        let cache_key = ReferenceCacheInputs {
            capture_key: key.clone(),
            reference_source_digest: self.source_digest.clone(),
            reference_binary_digest: self.reference_digest.clone(),
            scenario_digest,
            font_family: self.font_family.clone(),
            device_pixel_ratio: 1.0,
            terminal_capability: key.terminal_tier.clone(),
            locale: std::env::var("LC_ALL").unwrap_or_else(|_| "C.UTF-8".to_owned()),
            browser_version: self.browser_version.clone(),
            xterm_version: self.xterm_version.clone(),
            node_pty_version: self.node_pty_version.clone(),
            comparator_schema: COMPARISON_RECEIPT_SCHEMA.to_owned(),
        }
        .digest()
        .map_err(|error| error.to_string())?;
        let capture_root = isolation.evidence_dir.join("capture");
        let mut command = CommandSpec::new(&self.executable)
            .args([
                "compare".into(),
                "--scenario".into(),
                key.scenario.clone().into(),
                "--reference-bin".into(),
                self.reference_bin.as_os_str().to_owned(),
                "--reference-receipt".into(),
                self.repo_root
                    .join(".omo/evidence/task-2-grok-build-tui-experiential-parity/receipt.json")
                    .into_os_string(),
                "--reference-root".into(),
                self.repo_root
                    .join("inspirations/grok-build")
                    .into_os_string(),
                "--harness-bin".into(),
                self.harness_bin.as_os_str().to_owned(),
                "--candidate-receipt".into(),
                self.candidate_receipt.as_os_str().to_owned(),
                "--evidence-dir".into(),
                capture_root.as_os_str().to_owned(),
                "--font-family".into(),
                self.font_family.clone().into(),
            ])
            .cwd(&self.repo_root)
            .env("TUI_FIDELITY_REFERENCE_CACHE", self.cache_root.as_os_str())
            .env("TUI_FIDELITY_REFERENCE_CACHE_KEY", &cache_key);
        if let Some(browser) = &self.browser_bin {
            command = command.args(["--browser-bin".into(), browser.as_os_str().to_owned()]);
        }
        if let Some(node_modules) = &self.node_modules {
            command = command.args(["--node-modules".into(), node_modules.as_os_str().to_owned()]);
        }
        self.run(command, isolation)
    }

    fn owner_test(&self, key: &str, isolation: &JobIsolation) -> Result<PathBuf, String> {
        let command = CommandSpec::new("cargo")
            .args(["nextest", "run", "-p", "harness-tui"])
            .cwd(&self.repo_root)
            .env("TUI_FIDELITY_OWNER_KEY", key);
        self.run(command, isolation)
    }

    fn reviewer(&self, key: &str, isolation: &JobIsolation) -> Result<PathBuf, String> {
        let source = self
            .cache_root
            .parent()
            .ok_or("evidence root has no parent")?
            .join("reviewer-receipts")
            .join(format!("{key}.json"));
        let input = fs::read(&source).map_err(|error| format!("{}: {error}", source.display()))?;
        let value: serde_json::Value =
            serde_json::from_slice(&input).map_err(|error| error.to_string())?;
        if value.get("verdict").and_then(serde_json::Value::as_str) != Some("APPROVE") {
            return Err(format!("reviewer receipt {key} is not APPROVE"));
        }
        let target = isolation.evidence_dir.join("reviewer-receipt.json");
        fs::write(&target, input).map_err(|error| error.to_string())?;
        Ok(target)
    }

    fn run(&self, command: CommandSpec, isolation: &JobIsolation) -> Result<PathBuf, String> {
        let remaining = self
            .profile
            .deadline()
            .checked_sub(self.started.elapsed())
            .unwrap_or(Duration::from_millis(1));
        let runner = DeadlineRunner::new(
            remaining,
            Duration::from_secs(2),
            ResourceLimits::verification_default(),
            self.interrupt.clone(),
        );
        let receipt = runner.run(&command).map_err(|error| error.to_string())?;
        let path = isolation.evidence_dir.join("command-receipt.json");
        let bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
        fs::write(&path, bytes).map_err(|error| error.to_string())?;
        if receipt.status == CommandStatus::Passed {
            Ok(path)
        } else {
            Err(format!("command {:?}: {}", receipt.status, receipt.stderr))
        }
    }
}

fn write_receipt(isolation: &JobIsolation, key: &str, kind: &str) -> Result<PathBuf, String> {
    let path = isolation.evidence_dir.join("static-receipt.json");
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": "harness.tui-fidelity.non-runtime.v1",
        "type": kind,
        "key": key,
        "status": "passed"
    }))
    .map_err(|error| error.to_string())?;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    hash_bytes(&bytes).map_err(|error| error.to_string())
}

fn package_version(node_modules: Option<&Path>, package: &str) -> Result<String, String> {
    let Some(node_modules) = node_modules else {
        return Ok("unavailable".to_owned());
    };
    let path = node_modules.join(package).join("package.json");
    let input = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&input).map_err(|error| error.to_string())?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{} has no version", path.display()))
}
