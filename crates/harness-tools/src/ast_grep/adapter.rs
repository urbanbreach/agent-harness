use std::path::Path;
use std::time::Duration;

use harness_core::tool::ToolError;
use serde::Deserialize;
use tokio::process::Command;
use tokio::time::timeout;

use super::{cap_text, SearchRoot};

const AST_GREP_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AstGrepCliMatch {
    pub(super) text: String,
    pub(super) range: AstGrepCliRange,
    pub(super) file: String,
    #[serde(default)]
    pub(super) lines: Option<String>,
    #[serde(default)]
    pub(super) language: Option<String>,
    #[serde(default)]
    pub(super) replacement: Option<String>,
    #[serde(default)]
    pub(super) replacement_offsets: Option<AstGrepCliByteRange>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AstGrepCliRange {
    #[serde(default)]
    pub(super) byte_offset: Option<AstGrepCliByteRange>,
    pub(super) start: AstGrepCliPosition,
    pub(super) end: AstGrepCliPosition,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct AstGrepCliByteRange {
    pub(super) start: u64,
    pub(super) end: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct AstGrepCliPosition {
    pub(super) line: u64,
    pub(super) column: u64,
}

#[derive(Debug)]
pub(super) struct AstGrepAdapterOutput {
    pub(super) matches: Vec<AstGrepCliMatch>,
    pub(super) warnings: Vec<String>,
    pub(super) exit_status: Option<i32>,
}

pub(super) struct AstGrepRunRequest<'a> {
    pub(super) workspace: &'a Path,
    pub(super) roots: &'a [SearchRoot],
    pub(super) pattern: &'a str,
    pub(super) rewrite: Option<&'a str>,
    pub(super) language: &'a str,
    pub(super) include: &'a [String],
    pub(super) exclude: &'a [String],
    pub(super) context: usize,
    pub(super) command_name: &'a str,
    pub(super) tool_id: &'a str,
}

pub(super) async fn run_ast_grep(
    request: AstGrepRunRequest<'_>,
) -> Result<AstGrepAdapterOutput, ToolError> {
    let mut command = Command::new(request.command_name);
    command
        .kill_on_drop(true)
        .current_dir(request.workspace)
        .arg("run")
        .arg("--pattern")
        .arg(request.pattern)
        .arg("--lang")
        .arg(request.language)
        .arg("--json=compact")
        .arg("--color")
        .arg("never")
        .arg("--heading")
        .arg("never")
        .arg("--context")
        .arg(request.context.to_string());
    if let Some(rewrite) = request.rewrite {
        command.arg("--rewrite").arg(rewrite);
    }
    for pattern in request.include {
        command.arg("--globs").arg(pattern);
    }
    for pattern in request.exclude {
        command.arg("--globs").arg(format!("!{pattern}"));
    }
    for root in request.roots {
        command.arg(&root.relative);
    }

    let output = timeout(Duration::from_secs(AST_GREP_TIMEOUT_SECS), command.output())
        .await
        .map_err(|_| {
            ToolError::Execution(format!(
                "ast-grep adapter timed out after {AST_GREP_TIMEOUT_SECS}s; narrow paths, include globs, or pattern"
            ))
        })?
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ToolError::Execution(format!(
                    "{} requires the `ast-grep` binary on PATH; install ast-grep or remove this tool from the active toolset",
                    request.tool_id
                ))
            } else {
                ToolError::Execution(format!("failed to run ast-grep adapter: {err}"))
            }
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if stderr.contains("Pattern contains an ERROR node") {
        return Err(ToolError::InvalidArguments(format!(
            "ast-grep could not parse pattern `{}` cleanly for language `{}`: {}",
            request.pattern,
            request.language,
            compact_message(&stderr)
        )));
    }

    let warnings = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(cap_warning)
        .collect::<Vec<_>>();
    let parsed = serde_json::from_str::<Vec<AstGrepCliMatch>>(stdout.trim()).map_err(|err| {
        let status = output.status.code();
        let stderr = compact_message(&stderr);
        ToolError::Execution(format!(
            "failed to parse ast-grep JSON output for language `{}` (status {status:?}): {err}; stderr: {stderr}",
            request.language
        ))
    })?;

    if !(output.status.success() || output.status.code() == Some(1) && parsed.is_empty()) {
        return Err(ToolError::Execution(format!(
            "ast-grep search failed for language `{}` with status {:?}: {}",
            request.language,
            output.status.code(),
            compact_message(&stderr)
        )));
    }

    Ok(AstGrepAdapterOutput {
        matches: parsed,
        warnings,
        exit_status: output.status.code(),
    })
}

fn cap_warning(line: &str) -> String {
    cap_text(line, 500).0
}

fn compact_message(message: &str) -> String {
    let compact = message
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.is_empty() {
        "<no stderr>".to_string()
    } else {
        cap_text(&compact, 1_000).0
    }
}
