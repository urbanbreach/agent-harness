//! Binary update check CLI surface (`harness update check`).
//!
//! Surfaces the real local-manifest update check from
//! [`harness_core::binary_update`] behind an operator command. Reads
//! `.agent-harness/update-manifest.json` from the workspace, compares versions,
//! and writes a durable receipt at `.agent-harness/update-check.receipt.json`.
//! Without a manifest, the check reports structured unavailability (never a
//! false "up to date"). No download, apply, or restart pipeline exists.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use harness_core::binary_update::{run_local_manifest_update_check, LocalManifestUpdateProduct};

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
}

#[derive(Debug, Args, Clone)]
struct UpdateCheckCommand {
    /// Workspace root containing `.agent-harness/update-manifest.json` (default: current directory).
    #[arg(long)]
    workspace: Option<PathBuf>,
}

pub(crate) fn execute_with_io(command: UpdateCommand, io: &mut CliIo<'_>, deps: &CliDeps) -> i32 {
    match command.command {
        UpdateSubcommand::Check(cmd) => run_check(cmd, io, deps),
    }
}

fn resolve_workspace_root(explicit: &Option<PathBuf>, deps: &CliDeps) -> PathBuf {
    explicit
        .clone()
        .unwrap_or_else(|| deps.current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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
}
