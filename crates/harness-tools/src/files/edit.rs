use super::{contents, digest, MAX_FILE};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WriteArgs {
    #[serde(rename = "filePath", alias = "file_path", alias = "path")]
    pub(super) path: PathBuf,
    content: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct EditArgs {
    #[serde(rename = "filePath", alias = "file_path", alias = "path")]
    pub(super) path: PathBuf,
    #[serde(rename = "oldString", alias = "old_string", alias = "old_text")]
    old: Option<String>,
    #[serde(rename = "newString", alias = "new_string", alias = "new_text")]
    new: Option<String>,
    #[serde(default, rename = "replaceAll", alias = "replace_all")]
    all: bool,
    #[serde(default)]
    edits: Vec<crate::hashline::Edit>,
}
pub(super) fn write(ctx: &ToolContext, args: WriteArgs) -> Result<ToolResult, ToolError> {
    let path = ctx.resolve_workspace_path(&args.path)?;
    ctx.tool_state.edit(&path, |previous| {
        let existing = existing_contents(&path)?;
        if let Some(old) = &existing {
            check_read(old, previous)?;
        }
        commit(
            ctx,
            &path,
            &args.path,
            existing.as_deref(),
            Some(args.content),
        )
    })
}
pub(super) fn edit(ctx: &ToolContext, args: EditArgs) -> Result<ToolResult, ToolError> {
    let path = ctx.resolve_workspace_path(&args.path)?;
    ctx.tool_state.edit(&path, |previous| {
        let existing = existing_contents(&path)?;
        if let Some(text) = &existing {
            check_read(text, previous)?;
        }
        let text = existing.as_deref().unwrap_or("");
        let edited = match (args.old, args.new, args.edits.is_empty()) {
            (Some(old), Some(new), true) if old.is_empty() && existing.is_none() => new,
            (Some(old), Some(new), true) if !old.is_empty() => {
                let old = line_endings(text, old);
                let new = line_endings(text, new);
                let count = text.matches(&old).count();
                if count == 0 || (count > 1 && !args.all) {
                    return Err(ToolError::InvalidArguments(format!(
                        "oldString matched {count} times; provide a unique match or replaceAll"
                    )));
                }
                text.replace(&old, &new)
            }
            (None, None, false) if !args.all => crate::hashline::apply(text, args.edits)?,
            _ => {
                return Err(ToolError::InvalidArguments(
                    "provide oldString/newString or line edits, without mixing the two".into(),
                ))
            }
        };
        commit(ctx, &path, &args.path, existing.as_deref(), Some(edited))
    })
}
pub(crate) fn existing_contents(path: &Path) -> Result<Option<String>, ToolError> {
    match contents(path) {
        Ok(text) => Ok(Some(text)),
        Err(ToolError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
pub(crate) fn check_read(text: &str, previous: Option<&str>) -> Result<(), ToolError> {
    if previous == Some(digest(text).as_str()) {
        Ok(())
    } else {
        Err(ToolError::Execution(
            "read the current file before editing; it is unread or has changed".into(),
        ))
    }
}
pub(crate) fn commit(
    ctx: &ToolContext,
    path: &Path,
    display: &Path,
    before: Option<&str>,
    text: Option<String>,
) -> Result<(ToolResult, String), ToolError> {
    commit_with_metadata(ctx, path, display, before, text, None, true)
}
pub(crate) fn commit_with_metadata(
    ctx: &ToolContext,
    path: &Path,
    display: &Path,
    before: Option<&str>,
    text: Option<String>,
    permissions: Option<fs::Permissions>,
    preserve_style: bool,
) -> Result<(ToolResult, String), ToolError> {
    if ctx.cancellation.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    let executor = tokio::runtime::Handle::current();
    let (text, format_warning) = if let Some(text) = text {
        let (text, warning) = executor.block_on(crate::formatters::format(ctx, path, text))?;
        (Some(text), warning)
    } else {
        (None, None)
    };
    let text = text.map(|text| {
        if !preserve_style {
            return text;
        }
        let mut text = match before {
            Some(old) => line_endings(old, text),
            None => text,
        };
        if before.is_some_and(|old| old.starts_with('\u{feff}')) && !text.starts_with('\u{feff}') {
            text.insert(0, '\u{feff}');
        }
        text
    });
    let deleting = text.is_none();
    let text = text.as_deref().unwrap_or("");
    if text.len() as u64 > MAX_FILE {
        return Err(ToolError::InvalidArguments(
            "edited file exceeds 8 MiB".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::InvalidArguments("file has no parent".into()))?;
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(200))
        .diff_lines(before.unwrap_or(""), text)
        .unified_diff()
        .header(
            &before.map_or_else(
                || "/dev/null".into(),
                |_| format!("a/{}", display.display()),
            ),
            &if deleting {
                "/dev/null".into()
            } else {
                format!("b/{}", display.display())
            },
        )
        .to_string();
    if ctx.cancellation.is_cancelled() {
        return Err(ToolError::Cancelled);
    }
    if existing_contents(path)?.as_deref() != before {
        return Err(ToolError::Execution(
            "file changed before the edit could be committed".into(),
        ));
    }
    let receipt = executor
        .block_on(ctx.coordinator.begin_tool_edit(
            ctx.tool_call_id.to_string(),
            path.into(),
            diff,
            (!deleting).then(|| digest(text)),
        ))
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    let written = if deleting {
        fs::remove_file(path).map_err(ToolError::Io).and_then(|()| {
            #[cfg(unix)]
            fs::File::open(parent)?.sync_all()?;
            Ok(())
        })
    } else {
        fs::create_dir_all(parent)
            .map_err(ToolError::Io)
            .and_then(|()| replace_file(path, text.as_bytes(), before.is_some(), permissions))
    };
    let hash = digest(&text);
    executor
        .block_on(
            ctx.coordinator.finish_tool_edit(
                ctx.tool_call_id.to_string(),
                receipt.edit_id.clone(),
                written
                    .as_ref()
                    .map(|()| hash.clone())
                    .map_err(ToString::to_string),
            ),
        )
        .map_err(|e| ToolError::Execution(e.to_string()))?;
    written?;
    Ok((
        ToolResult::structured_with_artifacts(
            format!(
                "{} {} ({} bytes).{}",
                if deleting { "Deleted" } else { "Updated" },
                display.display(),
                text.len(),
                format_warning
                    .as_ref()
                    .map_or_else(String::new, |warning| format!(
                        "\nFormatter warning: {warning}"
                    ))
            ),
            json!({"path": display, "operation":if deleting { "delete" } else { "write" }, "bytes_written": text.len(), "digest": hash, "edit_id":receipt.edit_id, "diff_path":receipt.diff.path,"format_warning":format_warning}),
            vec![receipt.diff],
        ),
        hash,
    ))
}
fn replace_file(
    path: &Path,
    bytes: &[u8],
    existed: bool,
    mode: Option<fs::Permissions>,
) -> Result<(), ToolError> {
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::InvalidArguments("file has no parent".into()))?;
    let permissions = if existed {
        Some(fs::metadata(path)?.permissions())
    } else {
        None
    };
    if permissions.as_ref().is_some_and(fs::Permissions::readonly) {
        return Err(ToolError::Execution("file is read-only".into()));
    }
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut temp = builder.tempfile_in(parent)?;
    temp.write_all(bytes)?;
    if let Some(permissions) = mode.or(permissions) {
        #[cfg(unix)]
        let permissions = {
            use std::os::unix::fs::PermissionsExt;
            fs::Permissions::from_mode(permissions.mode() & 0o777)
        };
        temp.as_file().set_permissions(permissions)?;
    }
    temp.as_file().sync_all()?;
    if existed {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    }
    .map_err(|error| ToolError::Io(error.error))?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
fn line_endings(old: &str, new: String) -> String {
    let crlf = old.contains("\r\n")
        && !old.starts_with('\n')
        && !old
            .as_bytes()
            .windows(2)
            .any(|w| w[1] == b'\n' && w[0] != b'\r');
    if crlf {
        new.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        new
    }
}
