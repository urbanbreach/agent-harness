use super::{
    session::{uri, Session},
    supports,
    wire::{failure, LIMIT},
};
use harness_core::{
    config::LspServerConfig,
    tool::{ToolContext, ToolError},
};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::time::Instant;

pub(super) async fn workspace(
    session: &mut Session,
    context: &ToolContext,
    config: &LspServerConfig,
    primary: PathBuf,
    deadline: Instant,
) -> Result<Value, ToolError> {
    let scan_context = context.clone();
    let extensions = config.extensions.clone().unwrap_or_default();
    let selected = primary.clone();
    // Always join filesystem work; cancellation/deadline checks run inside the scan.
    let mut files =
        tokio::task::spawn_blocking(move || scan(&scan_context, extensions, selected, deadline))
            .await
            .map_err(|_| failure("diagnostic workspace scan stopped"))??;
    let count = files.len();
    files.retain(|path| path != &primary);
    let mut files = context
        .coordinator
        .allowed_paths(context.tool_call_id.to_string(), files, &["read", "lsp"])
        .await
        .map_err(|e| failure(&e.to_string()))?;
    // The primary document already passed this tool call's approval, including one-time grants.
    if !files.contains(&primary) {
        files.push(primary);
    }
    files.sort();
    files.dedup();
    let skipped = count - files.len();
    let (mut bytes, mut diagnostic_count) = (0, 0);
    let mut reports = Vec::new();
    let pull = supports(&session.capabilities, "diagnosticProvider");
    for path in files {
        check(context, deadline)?;
        let read_context = context.clone();
        let source = path.clone();
        let text = tokio::task::spawn_blocking(move || {
            let resolved = read_context.resolve_workspace_path(&source)?;
            if resolved != source || resolved.metadata()?.len() > 512 * 1024 {
                return Err(failure("LSP source changed path or exceeds 512 KiB"));
            }
            let text = crate::files::contents(&resolved)?;
            if text.len() > 512 * 1024 {
                return Err(failure("LSP source exceeds 512 KiB"));
            }
            Ok(text)
        })
        .await
        .map_err(|_| failure("LSP source reader stopped"))??;
        let uri = uri(&path)?;
        let diagnostics = tokio::select! {
            biased;
            () = context.cancellation.cancelled() => return Err(ToolError::Cancelled),
            result = tokio::time::timeout_at(deadline, async {
                session.document(&uri, &super::language(&path), &text).await?;
                if pull {
                    let result = session.request("textDocument/diagnostic", json!({"textDocument":{"uri":uri}})).await?;
                    Ok(result.get("items").cloned().unwrap_or(Value::Null))
                } else { session.published_diagnostics(&uri).await }
            }) => result.map_err(|_| failure("LSP workspace diagnostics timed out"))??,
        };
        diagnostic_count += diagnostics
            .as_array()
            .ok_or_else(|| failure("LSP returned invalid file diagnostics"))?
            .len();
        let report = json!({"filePath":path,"diagnostics":diagnostics});
        bytes += report.to_string().len();
        if bytes > LIMIT {
            return Err(failure("LSP workspace diagnostics exceed 4 MiB"));
        }
        reports.push(report);
    }
    Ok(
        json!({"scope":"workspace","workspaceRoot":context.workspace_root,"filesScanned":reports.len(),"skippedFiles":skipped,"complete":skipped == 0,"diagnosticCount":diagnostic_count,"reports":reports}),
    )
}
fn scan(
    context: &ToolContext,
    extensions: Vec<String>,
    primary: PathBuf,
    deadline: Instant,
) -> Result<Vec<PathBuf>, ToolError> {
    let mut files = vec![primary];
    let walk = walkdir::WalkDir::new(&context.workspace_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_dir()
                || !matches!(
                    entry.file_name().to_str(),
                    Some(
                        ".git"
                            | ".jj"
                            | ".hg"
                            | "target"
                            | "node_modules"
                            | "__pycache__"
                            | ".harness"
                    )
                )
        });
    for (index, entry) in walk.enumerate() {
        check(context, deadline)?;
        if index == 100_000 {
            return Err(failure(
                "diagnostic workspace exceeds 100,000 entries; use fileDiagnostics",
            ));
        }
        let entry = entry.map_err(|e| failure(&e.to_string()))?;
        if !entry.file_type().is_file() || files[0] == entry.path() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");
        if extensions
            .iter()
            .any(|ext| ext.trim_start_matches('.').eq_ignore_ascii_case(extension))
        {
            if files.len() == 200 {
                return Err(failure(
                    "workspace diagnostics exceeds 200 files; use fileDiagnostics",
                ));
            }
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}
fn check(context: &ToolContext, deadline: Instant) -> Result<(), ToolError> {
    if context.cancellation.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    if deadline <= Instant::now() {
        return Err(failure("LSP workspace diagnostics timed out"));
    }
    Ok(())
}
