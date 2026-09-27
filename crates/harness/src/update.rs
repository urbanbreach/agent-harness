use crate::{CliDeps, CliIo};
use harness_core::binary_update::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct UpdateCommand {
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    action: Action,
}
#[derive(clap::Subcommand)]
enum Action {
    Check,
    Download {
        #[arg(long)]
        url: String,
        #[arg(long)]
        expected_sha256: Option<String>,
        #[arg(long)]
        dest_dir: Option<PathBuf>,
    },
    Apply {
        #[arg(long)]
        artifact_path: PathBuf,
        #[arg(long)]
        target: Option<PathBuf>,
    },
    Restart {
        #[arg(long)]
        target: Option<PathBuf>,
    },
    Run {
        #[arg(long)]
        target: Option<PathBuf>,
    },
}
pub(crate) fn execute(
    command: UpdateCommand,
    io: &mut CliIo<'_>,
    deps: &CliDeps,
) -> Result<(), String> {
    let root = crate::workspace::workspace_root(command.workspace, deps)?;
    match command.action {
        Action::Check => {
            let check = run_local_manifest_update_check(&root, None).map_err(|e| e.to_string())?;
            report(io, &product(&check), check.check.is_checked())
        }
        Action::Download {
            url,
            expected_sha256,
            dest_dir,
        } => {
            let directory =
                root.join(dest_dir.unwrap_or_else(|| ".agent-harness/downloads".into()));
            let download = download_update_artifact(&url, expected_sha256.as_deref(), &directory);
            report(
                io,
                &json!({"download":download,"one_line":download.one_line()}),
                download.is_downloaded(),
            )
        }
        Action::Apply {
            artifact_path,
            target: explicit,
        } => {
            let apply = apply_update(&root.join(artifact_path), &target(explicit, &root)?);
            report(
                io,
                &json!({"apply":apply,"one_line":apply.one_line()}),
                apply.is_applied(),
            )
        }
        Action::Restart { target: explicit } => restart(&target(explicit, &root)?, None, &root, io),
        Action::Run { target: explicit } => pipeline(&root, &target(explicit, &root)?, io),
    }
}
fn target(explicit: Option<PathBuf>, root: &Path) -> Result<PathBuf, String> {
    explicit.map_or_else(
        || std::env::current_exe().map_err(|e| e.to_string()),
        |path| Ok(root.join(path)),
    )
}
fn product(product: &LocalManifestUpdateProduct) -> Value {
    json!({"check":product.check,"summary":product.summary,"receipt_path":product.receipt_path,
        "manifest_path":product.manifest_path,"version":product.version,"one_line":product.one_line()})
}
fn report(io: &mut CliIo<'_>, value: &Value, success: bool) -> Result<(), String> {
    crate::inspect::print_json(io, value)?;
    if success {
        Ok(())
    } else {
        Err("update operation did not succeed; see its report".into())
    }
}
fn restart(
    target: &Path,
    version: Option<&str>,
    root: &Path,
    io: &mut CliIo<'_>,
) -> Result<(), String> {
    io.stdout.flush().map_err(|e| e.to_string())?;
    io.stderr.flush().map_err(|e| e.to_string())?;
    let restart = restart_after_update(target, version, root);
    report(
        io,
        &json!({"restart":restart,"one_line":restart.one_line()}),
        false,
    )
}
fn pipeline(root: &Path, target: &Path, io: &mut CliIo<'_>) -> Result<(), String> {
    let check = run_local_manifest_update_check(root, None).map_err(|e| e.to_string())?;
    if !check.check.is_update_available() {
        return report(io, &product(&check), check.check.is_checked());
    }
    let manifest = check
        .manifest
        .as_ref()
        .ok_or("checked update manifest is missing")?;
    let url = manifest
        .download_url
        .as_deref()
        .ok_or("update manifest has no download_url")?;
    let download = download_update_artifact(
        url,
        manifest.sha256.as_deref(),
        &root.join(".agent-harness/downloads"),
    );
    let BinaryUpdateDownload::Downloaded { artifact_path, .. } = &download else {
        return report(io, &json!({"check":check.check,"download":download}), false);
    };
    let apply = apply_update(Path::new(artifact_path), target);
    report(
        io,
        &json!({"check":check.check,"download":download,"apply":apply}),
        apply.is_applied(),
    )?;
    restart(target, Some(&manifest.version), root, io)
}
