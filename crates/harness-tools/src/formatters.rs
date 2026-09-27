mod discovery;
use harness_core::tool::{ToolContext, ToolError};
use std::{io::Read, path::Path, time::Duration};

pub(crate) async fn format(
    context: &ToolContext,
    path: &Path,
    mut text: String,
) -> Result<(String, Option<String>), ToolError> {
    if text.len() as u64 > crate::files::MAX_FILE {
        return Err(ToolError::InvalidArguments(
            "edited file exceeds 8 MiB".into(),
        ));
    }
    let formatters = discovery::resolve(context, path).await?;
    let mut warnings = Vec::new();
    for formatter in formatters {
        match apply(context, path, &text, &formatter).await {
            Ok(formatted) => text = formatted,
            Err(ToolError::Cancelled) => return Err(ToolError::Cancelled),
            Err(error) => warnings.push(format!(
                "{}: {}",
                formatter.name,
                context.redactor.redact_text(&error.to_string())
            )),
        }
    }
    let warning = (!warnings.is_empty()).then(|| warnings.join("\n"));
    Ok((text, warning))
}
async fn apply(
    context: &ToolContext,
    path: &Path,
    text: &str,
    formatter: &discovery::Invocation,
) -> Result<String, ToolError> {
    if formatter.command.is_empty() {
        return Err(ToolError::InvalidArguments(
            "formatter command is empty".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::InvalidArguments("file has no parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let directory = tempfile::Builder::new()
        .prefix(".harness-format-")
        .tempdir_in(parent)?;
    let staged =
        directory.path().join(path.file_name().ok_or_else(|| {
            ToolError::InvalidArguments("formatter target has no filename".into())
        })?);
    std::fs::write(&staged, text.as_bytes())?;
    let target = staged
        .to_str()
        .ok_or_else(|| ToolError::InvalidArguments("formatter path must be UTF-8".into()))?;
    let mut args: Vec<_> = formatter
        .command
        .iter()
        .map(|arg| arg.replace("$FILE", target))
        .collect();
    if !formatter.command.iter().any(|arg| arg.contains("$FILE")) {
        args.push(target.into());
    }
    let (program, args) = args
        .split_first()
        .filter(|(program, _)| !program.is_empty())
        .ok_or_else(|| ToolError::InvalidArguments("formatter command is empty".into()))?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(&context.workspace_root)
        .env("HARNESS_FORMATTER", "1")
        .envs(&formatter.environment);
    let output =
        crate::process::run(command, Duration::from_secs(30), &context.cancellation).await?;
    if !output.status.success() {
        let reason = if output.stderr.is_empty() {
            output.stdout
        } else {
            output.stderr
        };
        let reason = String::from_utf8_lossy(&reason[..reason.len().min(4096)]);
        return Err(ToolError::Execution(format!(
            "formatter failed ({}): {reason}",
            output.status
        )));
    }
    let file = harness_core::store::open_private_file(&staged)?;
    if file.metadata()?.len() > crate::files::MAX_FILE {
        return Err(ToolError::Execution(
            "formatter output exceeds 8 MiB".into(),
        ));
    }
    let mut result = String::new();
    file.take(crate::files::MAX_FILE + 1)
        .read_to_string(&mut result)?;
    if result.len() as u64 > crate::files::MAX_FILE {
        return Err(ToolError::Execution(
            "formatter output exceeds 8 MiB".into(),
        ));
    }
    Ok(result)
}
