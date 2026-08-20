//! Binary update CLI surface (`harness update check|download|apply|restart|run`).
//!
//! Surfaces the real local-manifest update check from
//! [`harness_core::binary_update`] behind an operator command. `check`
//! compares versions and writes a durable receipt. `download`, `apply`,
//! `restart`, and `run` complete the update pipeline:
//!
//! - `check` — compare against `.agent-harness/update-manifest.json`
//! - `download` — fetch an artifact from a URL (file:// or HTTP via curl)
//! - `apply` — replace the target binary with a downloaded artifact (backup + rollback)
//! - `restart` — exec the updated binary, replacing the current process (Unix only)
//! - `run` — full pipeline: check → download → apply → restart

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use harness_core::binary_update::{
    apply_update, download_update_artifact, load_local_update_manifest, restart_after_update,
    run_local_manifest_update_check, BinaryUpdateApply, BinaryUpdateDownload, BinaryUpdateRestart,
    LocalManifestUpdateProduct,
};

use crate::{CliDeps, CliIo};

#[derive(Debug, Args, Clone)]
pub(crate) struct UpdateCommand {
    #[command(subcommand)]
    command: UpdateSubcommand,
}

#[derive(Debug, Subcommand, Clone)]
enum UpdateSubcommand {
    /// Check for updates against a local manifest and write a durable receipt.
    Check(UpdateCheckCommand),
    /// Download an update artifact from a URL to a local directory.
    Download(UpdateDownloadCommand),
    /// Apply a downloaded artifact by replacing the target binary (backup + rollback).
    Apply(UpdateApplyCommand),
    /// Restart the process by exec-ing the updated binary (Unix only).
    Restart(UpdateRestartCommand),
    /// Run the full pipeline: check → download → apply → restart.
    Run(UpdateRunCommand),
}

#[derive(Debug, Args, Clone)]
struct UpdateCheckCommand {
    /// Workspace root containing `.agent-harness/update-manifest.json` (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct UpdateDownloadCommand {
    /// URL to download the artifact from (http/https or file://).
    #[arg(long)]
    url: String,
    /// Expected SHA-256 hash of the artifact (hex-encoded). If provided, the
    /// download is verified and rejected on mismatch.
    #[arg(long)]
    expected_sha256: Option<String>,
    /// Destination directory for the downloaded artifact (default: workspace `.agent-harness/downloads`).
    #[arg(long)]
    dest_dir: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct UpdateApplyCommand {
    /// Path to the downloaded artifact to apply.
    #[arg(long)]
    artifact_path: PathBuf,
    /// Target binary path to replace (default: current executable).
    #[arg(long)]
    target: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct UpdateRestartCommand {
    /// Target binary path to exec (default: current executable).
    #[arg(long)]
    target: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct UpdateRunCommand {
    /// Workspace root containing `.agent-harness/update-manifest.json` (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Target binary path to replace and restart (default: current executable).
    #[arg(long)]
    target: Option<PathBuf>,
}

pub(crate) fn execute_with_io(command: UpdateCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        UpdateSubcommand::Check(cmd) => run_check(cmd, io, deps),
        UpdateSubcommand::Download(cmd) => run_download(cmd, io, deps),
        UpdateSubcommand::Apply(cmd) => run_apply(cmd, io, deps),
        UpdateSubcommand::Restart(cmd) => run_restart(cmd, io, deps),
        UpdateSubcommand::Run(cmd) => run_pipeline(cmd, io, deps),
    }
}

fn resolve_workspace_root(explicit: &Option<PathBuf>, deps: &CliDeps) -> PathBuf {
    explicit
        .clone()
        .unwrap_or_else(|| deps.current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn resolve_target(explicit: Option<&PathBuf>) -> PathBuf {
    explicit
        .cloned()
        .unwrap_or_else(|| std::env::current_exe().unwrap_or_else(|_| PathBuf::from("harness")))
}

fn run_check(cmd: UpdateCheckCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&cmd.workspace, deps);
    match run_local_manifest_update_check(&root, None) {
        Ok(product) => write_product(io, &product),
        Err(err) => {
            let _ = writeln!(io.stderr, "update check: {err}");
            1
        }
    }
}

fn run_download(cmd: UpdateDownloadCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let dest_dir = cmd
        .dest_dir
        .clone()
        .unwrap_or_else(|| resolve_workspace_root(&None, deps).join(".agent-harness/downloads"));
    let result = download_update_artifact(&cmd.url, cmd.expected_sha256.as_deref(), &dest_dir);
    write_download(io, &result)
}

fn run_apply(cmd: UpdateApplyCommand, io: &mut CliIo<'_>, _deps: &CliDeps) -> i32 {
    let target = resolve_target(cmd.target.as_ref());
    let result = apply_update(&cmd.artifact_path, &target);
    write_apply(io, &result)
}

fn run_restart(cmd: UpdateRestartCommand, io: &mut CliIo<'_>, _deps: &CliDeps) -> i32 {
    let target = resolve_target(cmd.target.as_ref());
    let signal = restart_after_update(&target, None);
    write_restart(io, &signal)
}

fn run_pipeline(cmd: UpdateRunCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    let root = resolve_workspace_root(&cmd.workspace, deps);
    let target = resolve_target(cmd.target.as_ref());

    // Step 1: check
    let product = match run_local_manifest_update_check(&root, None) {
        Ok(product) => product,
        Err(err) => {
            let _ = writeln!(io.stderr, "update run: check failed: {err}");
            return 1;
        }
    };

    if !product.summary.update_available {
        return write_product(io, &product);
    }

    // Step 2: download (using manifest's download_url + sha256)
    let manifest = match load_local_update_manifest(&product.manifest_path) {
        Ok(m) => m,
        Err(err) => {
            let _ = writeln!(io.stderr, "update run: manifest load failed: {err}");
            return 1;
        }
    };

    let download_url = match &manifest.download_url {
        Some(url) => url.as_str(),
        None => {
            let _ = writeln!(
                io.stderr,
                "update run: manifest has no download_url; cannot download"
            );
            return 1;
        }
    };

    let dest_dir = root.join(".agent-harness/downloads");
    let download = download_update_artifact(download_url, manifest.sha256.as_deref(), &dest_dir);
    if !download.is_downloaded() {
        let _ = writeln!(
            io.stderr,
            "update run: download failed: {}",
            download.one_line()
        );
        return 1;
    }

    let artifact_path = match &download {
        BinaryUpdateDownload::Downloaded { artifact_path, .. } => artifact_path.clone(),
        BinaryUpdateDownload::Unavailable { .. } => {
            let _ = writeln!(
                io.stderr,
                "update run: download unexpectedly unavailable after check"
            );
            return 1;
        }
    };

    // Step 3: apply (backup + rollback handled by apply_update)
    let apply = apply_update(Path::new(&artifact_path), &target);
    if !apply.is_applied() {
        let _ = writeln!(io.stderr, "update run: apply failed: {}", apply.one_line());
        return 1;
    }

    // Step 4: restart (if exec succeeds, we never reach the return)
    let restart = restart_after_update(&target, Some(&manifest.version));

    let output = serde_json::json!({
        "check": product.check,
        "download": download,
        "apply": apply,
        "restart": restart,
        "one_line": format!(
            "{} | {} | {} | {}",
            product.check.one_line(),
            download.one_line(),
            apply.one_line(),
            restart.one_line(),
        ),
    });
    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "update run: failed to serialize JSON: {err}");
            1
        }
    }
}

fn write_product(io: &mut CliIo<'_>, product: &LocalManifestUpdateProduct) -> i32 {
    let output = serde_json::json!({
        "check": product.check,
        "summary": product.summary,
        "receipt_path": product.receipt_path.display().to_string(),
        "manifest_path": product.manifest_path.display().to_string(),
        "version": product.version,
        "one_line": product.one_line(),
    });
    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "update check: failed to serialize JSON: {err}");
            1
        }
    }
}

fn write_download(io: &mut CliIo<'_>, result: &BinaryUpdateDownload) -> i32 {
    let output = serde_json::json!({
        "download": result,
        "one_line": result.one_line(),
    });
    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            if result.is_downloaded() {
                0
            } else {
                2
            }
        }
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "update download: failed to serialize JSON: {err}"
            );
            1
        }
    }
}

fn write_apply(io: &mut CliIo<'_>, result: &BinaryUpdateApply) -> i32 {
    let output = serde_json::json!({
        "apply": result,
        "one_line": result.one_line(),
    });
    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            if result.is_applied() {
                0
            } else {
                2
            }
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "update apply: failed to serialize JSON: {err}");
            1
        }
    }
}

fn write_restart(io: &mut CliIo<'_>, signal: &BinaryUpdateRestart) -> i32 {
    let output = serde_json::json!({
        "restart": signal,
        "one_line": signal.one_line(),
    });
    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            let _ = writeln!(io.stdout, "{json}");
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "update restart: failed to serialize JSON: {err}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliIo;
    use std::io::Cursor;
    use tempfile::tempdir;

    fn run_cli(workspace: &std::path::Path, args: &[&str]) -> (i32, String, String) {
        let mut stdin = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
        let deps = CliDeps::real().with_current_dir(workspace.to_path_buf());
        let mut argv: Vec<String> = vec!["harness".to_string(), "update".to_string()];
        for arg in args {
            argv.push((*arg).to_string());
        }
        let outcome = crate::run(argv, &mut io, deps);
        (
            outcome.code,
            String::from_utf8_lossy(&stdout).to_string(),
            String::from_utf8_lossy(&stderr).to_string(),
        )
    }

    #[test]
    fn update_check_missing_manifest_reports_unavailable_with_receipt() {
        // arrange — empty workspace, no manifest
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act
        let (code, stdout, stderr) = run_cli(ws, &["check"]);

        // assert — unavailable (no manifest), but receipt written
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["check"]["status"].as_str(),
            Some("unavailable"),
            "stdout: {stdout}"
        );
        assert!(
            ws.join(".agent-harness/update-check.receipt.json")
                .is_file(),
            "receipt must be written even without a manifest"
        );
    }

    #[test]
    fn update_check_with_newer_manifest_reports_update_available_and_receipt() {
        // arrange — workspace with a manifest advertising a newer version
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let ah = ws.join(".agent-harness");
        std::fs::create_dir_all(&ah).unwrap();
        std::fs::write(
            ah.join("update-manifest.json"),
            r#"{"version": "99.0.0", "channel": "stable"}"#,
        )
        .unwrap();

        // act
        let (code, stdout, stderr) = run_cli(ws, &["check"]);

        // assert — update available + durable receipt
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["check"]["status"].as_str(),
            Some("update_available"),
            "stdout: {stdout}"
        );
        assert_eq!(
            output["check"]["channel_version"].as_str(),
            Some("99.0.0"),
            "stdout: {stdout}"
        );
        assert!(
            ws.join(".agent-harness/update-check.receipt.json")
                .is_file(),
            "receipt must be written"
        );
    }

    #[test]
    fn update_check_with_current_manifest_reports_up_to_date() {
        // arrange — workspace with a manifest at the running version
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let ah = ws.join(".agent-harness");
        std::fs::create_dir_all(&ah).unwrap();
        let version = harness_core::binary_update::current_binary_version().version;
        std::fs::write(
            ah.join("update-manifest.json"),
            format!(r#"{{"version": "{version}", "channel": "stable"}}"#),
        )
        .unwrap();

        // act
        let (code, stdout, stderr) = run_cli(ws, &["check"]);

        // assert — up to date
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["check"]["status"].as_str(),
            Some("up_to_date"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn update_download_from_file_url_succeeds() {
        // arrange — a local file to use as the artifact source
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let artifact_source = ws.join("new-binary");
        std::fs::write(&artifact_source, b"fake binary content").unwrap();
        let dest_dir = ws.join("downloads");

        // act
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "download",
                "--url",
                &format!("file://{}", artifact_source.display()),
                "--dest-dir",
                &dest_dir.display().to_string(),
            ],
        );

        // assert — downloaded successfully
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["download"]["status"].as_str(),
            Some("downloaded"),
            "stdout: {stdout}"
        );
        let artifact_path = output["download"]["artifact_path"]
            .as_str()
            .expect("artifact_path");
        assert!(Path::new(artifact_path).is_file());
        assert_eq!(output["download"]["bytes"].as_u64(), Some(19));
    }

    #[test]
    fn update_download_fails_on_empty_url() {
        // arrange — empty URL
        let dir = tempdir().unwrap();
        let ws = dir.path();

        // act
        let (code, stdout, _stderr) = run_cli(ws, &["download", "--url", ""]);

        // assert — unavailable
        assert_eq!(code, 2);
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["download"]["status"].as_str(),
            Some("unavailable"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn update_download_verifies_sha256_when_provided() {
        // arrange — a local file with a known SHA-256
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let artifact_source = ws.join("new-binary");
        let content = b"verified content";
        std::fs::write(&artifact_source, content).unwrap();
        let dest_dir = ws.join("downloads");

        // compute real sha256
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content);
        let digest = hasher.finalize();
        let mut expected_hash = String::with_capacity(digest.len() * 2);
        use std::fmt::Write;
        for b in digest.iter() {
            let _ = write!(&mut expected_hash, "{b:02x}");
        }

        // act
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "download",
                "--url",
                &format!("file://{}", artifact_source.display()),
                "--expected-sha256",
                &expected_hash,
                "--dest-dir",
                &dest_dir.display().to_string(),
            ],
        );

        // assert — downloaded with sha256_verified=true
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["download"]["status"].as_str(),
            Some("downloaded"),
            "stdout: {stdout}"
        );
        assert_eq!(output["download"]["sha256_verified"].as_bool(), Some(true));
    }

    #[test]
    fn update_download_rejects_mismatched_sha256() {
        // arrange — a local file with a wrong SHA-256
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let artifact_source = ws.join("new-binary");
        std::fs::write(&artifact_source, b"content").unwrap();
        let dest_dir = ws.join("downloads");

        // act
        let (code, stdout, _stderr) = run_cli(
            ws,
            &[
                "download",
                "--url",
                &format!("file://{}", artifact_source.display()),
                "--expected-sha256",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "--dest-dir",
                &dest_dir.display().to_string(),
            ],
        );

        // assert — unavailable (sha256 mismatch)
        assert_eq!(code, 2);
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["download"]["status"].as_str(),
            Some("unavailable"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn update_apply_replaces_binary_with_backup() {
        // arrange — a target binary and a downloaded artifact
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let target = ws.join("harness");
        std::fs::write(&target, b"old binary").unwrap();
        let artifact = ws.join("new-harness");
        std::fs::write(&artifact, b"new binary").unwrap();

        // act
        let (code, stdout, stderr) = run_cli(
            ws,
            &[
                "apply",
                "--artifact-path",
                &artifact.display().to_string(),
                "--target",
                &target.display().to_string(),
            ],
        );

        // assert — applied, target replaced, backup created
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["apply"]["status"].as_str(),
            Some("applied"),
            "stdout: {stdout}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new binary");
        let backup = target.with_extension("bak");
        assert!(backup.is_file());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "old binary");
    }

    #[test]
    fn update_apply_fails_when_artifact_missing() {
        // arrange — target exists but artifact does not
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let target = ws.join("harness");
        std::fs::write(&target, b"old binary").unwrap();
        let artifact = ws.join("nonexistent");

        // act
        let (code, stdout, _stderr) = run_cli(
            ws,
            &[
                "apply",
                "--artifact-path",
                &artifact.display().to_string(),
                "--target",
                &target.display().to_string(),
            ],
        );

        // assert — failed, target unchanged
        assert_eq!(code, 2);
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["apply"]["status"].as_str(),
            Some("failed"),
            "stdout: {stdout}"
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old binary");
    }

    #[test]
    fn update_restart_reports_exec_error_for_nonexistent_target() {
        // arrange — a target path that does not exist
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let target = ws.join("nonexistent-harness");

        // act
        let (code, stdout, _stderr) =
            run_cli(ws, &["restart", "--target", &target.display().to_string()]);

        // assert — restart signal with exec_error set
        assert_eq!(code, 0);
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(output["restart"]["restart_needed"].as_bool(), Some(true));
        assert!(
            output["restart"]["exec_error"].as_str().is_some(),
            "exec_error should be set: {stdout}"
        );
    }

    #[test]
    fn update_run_full_pipeline_with_update_available() {
        // arrange — workspace with a manifest advertising a newer version + file:// download URL.
        // The artifact uses a shebang pointing to a non-existent interpreter so that
        // exec fails with ENOENT instead of falling back to /bin/sh (which would
        // replace the test process).
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let artifact_source = ws.join("new-harness-artifact");
        std::fs::write(&artifact_source, b"#!/nonexistent/interpreter\n").unwrap();

        let ah = ws.join(".agent-harness");
        std::fs::create_dir_all(&ah).unwrap();
        let manifest = serde_json::json!({
            "version": "99.0.0",
            "channel": "stable",
            "download_url": format!("file://{}", artifact_source.display()),
        });
        std::fs::write(
            ah.join("update-manifest.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        // fake current binary to be replaced
        let target_binary = ws.join("harness");
        std::fs::write(&target_binary, b"old binary content").unwrap();

        // act
        let (code, stdout, stderr) = run_cli(
            ws,
            &["run", "--target", &target_binary.display().to_string()],
        );

        // assert — pipeline completed: check=update_available, download=downloaded, apply=applied
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["check"]["status"].as_str(),
            Some("update_available"),
            "stdout: {stdout}"
        );
        assert_eq!(
            output["download"]["status"].as_str(),
            Some("downloaded"),
            "stdout: {stdout}"
        );
        assert_eq!(
            output["apply"]["status"].as_str(),
            Some("applied"),
            "stdout: {stdout}"
        );
        assert_eq!(
            output["restart"]["restart_needed"].as_bool(),
            Some(true),
            "stdout: {stdout}"
        );
        assert!(
            output["restart"]["exec_error"].as_str().is_some(),
            "exec_error should be set since interpreter does not exist: {stdout}"
        );
        // target binary was replaced
        assert_eq!(
            std::fs::read_to_string(&target_binary).unwrap(),
            "#!/nonexistent/interpreter\n"
        );
        // backup was created
        let backup = target_binary.with_extension("bak");
        assert!(backup.is_file());
        assert_eq!(
            std::fs::read_to_string(&backup).unwrap(),
            "old binary content"
        );
    }

    #[test]
    fn update_run_stops_when_up_to_date() {
        // arrange — manifest at current version (no update available)
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let ah = ws.join(".agent-harness");
        std::fs::create_dir_all(&ah).unwrap();
        let version = harness_core::binary_update::current_binary_version().version;
        std::fs::write(
            ah.join("update-manifest.json"),
            format!(r#"{{"version": "{version}", "channel": "stable"}}"#),
        )
        .unwrap();

        // act
        let (code, stdout, stderr) = run_cli(ws, &["run"]);

        // assert — up to date, pipeline did not proceed to download/apply/restart
        assert_eq!(code, 0, "stderr: {stderr}");
        let output: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        assert_eq!(
            output["check"]["status"].as_str(),
            Some("up_to_date"),
            "stdout: {stdout}"
        );
        assert!(
            output.get("download").is_none(),
            "download should not be present when up to date"
        );
    }

    #[test]
    fn update_run_fails_when_manifest_has_no_download_url() {
        // arrange — manifest with newer version but no download_url
        let dir = tempdir().unwrap();
        let ws = dir.path();
        let ah = ws.join(".agent-harness");
        std::fs::create_dir_all(&ah).unwrap();
        std::fs::write(
            ah.join("update-manifest.json"),
            r#"{"version": "99.0.0", "channel": "stable"}"#,
        )
        .unwrap();

        // act
        let (code, _stdout, stderr) = run_cli(ws, &["run"]);

        // assert — fails with error about missing download_url
        assert_eq!(code, 1, "stderr: {stderr}");
        assert!(
            stderr.contains("download_url"),
            "stderr should mention download_url: {stderr}"
        );
    }
}
