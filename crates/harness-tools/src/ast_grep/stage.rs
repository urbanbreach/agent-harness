use super::*;
use std::{collections::BTreeSet, path::Path};

pub(super) struct Source {
    pub path: PathBuf,
    pub staged: PathBuf,
    pub before: String,
}
pub(super) fn prepare(
    ctx: &ToolContext,
    args: &Args,
    executor: &tokio::runtime::Handle,
) -> Result<(tempfile::TempDir, Vec<Source>, &'static str, usize), ToolError> {
    let include = globs(&args.include)?;
    let exclude = globs(&args.exclude)?;
    let explicit = args
        .language
        .as_deref()
        .map(|s| language(s).ok_or_else(|| invalid("unsupported language")))
        .transpose()?;
    let mut candidates = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let session_dir = ctx.artifacts_dir.parent().and_then(Path::parent);
    for input in args.roots() {
        let root = ctx.resolve_workspace_path(&input)?;
        if !root.starts_with(&ctx.workspace_root) {
            return Err(invalid("AST search paths must stay within the workspace"));
        }
        if root.is_file() {
            selected.insert(root.clone());
        }
        for path in crate::search::files(&root) {
            if ctx.cancellation.is_cancelled() {
                return Err(ToolError::Cancelled);
            }
            let path = path?;
            let relative = path.strip_prefix(&ctx.workspace_root).unwrap_or(&path);
            let supported = path.extension().and_then(|s| s.to_str()).and_then(language);
            if supported.is_none()
                || explicit.is_some_and(|l| Some(l) != supported)
                || session_dir.is_some_and(|dir| path.starts_with(dir))
                || (!args.include.is_empty() && !include.is_match(relative))
                || exclude.is_match(relative)
            {
                continue;
            }
            candidates.insert(path);
            if candidates.len() > 200 {
                return Err(failure(
                    "structural search exceeds 200 files; narrow the paths",
                ));
            }
        }
    }
    let count = candidates.len();
    let (selected, remaining): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|path| selected.contains(path));
    let mut paths = executor
        .block_on(
            ctx.coordinator
                .allowed_read_paths(ctx.tool_call_id.to_string(), remaining),
        )
        .map_err(|e| failure(e.to_string()))?;
    paths.extend(selected);
    paths.sort();
    paths.dedup();
    let mut skipped = count - paths.len();
    let languages: BTreeSet<_> = paths
        .iter()
        .filter_map(|p| p.extension().and_then(|s| s.to_str()).and_then(language))
        .collect();
    let language = explicit
        .or_else(|| {
            (languages.len() == 1)
                .then(|| languages.first().copied())
                .flatten()
        })
        .ok_or_else(|| {
            invalid(
                "language is required when paths contain no supported files or multiple languages",
            )
        })?;
    let stage = tempfile::Builder::new().prefix("harness-ast-").tempdir()?;
    let mut sources = Vec::new();
    let mut size = 0usize;
    for path in paths {
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if ctx.resolve_workspace_path(&path)? != path {
            return Err(failure("AST search path changed during discovery"));
        }
        let before = match crate::files::contents(&path) {
            Ok(text) if !text.contains('\0') => text,
            _ => {
                skipped += 1;
                continue;
            }
        };
        size += before.len();
        if size > 32 * 1024 * 1024 {
            return Err(failure(
                "structural search exceeds 32 MiB; narrow the paths",
            ));
        }
        let staged = stage.path().join(format!(
            "{}.{}",
            sources.len(),
            path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
        ));
        std::fs::write(&staged, &before)?;
        sources.push(Source {
            path,
            staged,
            before,
        });
    }
    Ok((stage, sources, language, skipped))
}
fn globs(patterns: &[String]) -> Result<globset::GlobSet, ToolError> {
    if patterns.len() > 64 {
        return Err(invalid("at most 64 include or exclude globs are allowed"));
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.len() > 8192 {
            return Err(invalid("glob exceeds 8192 bytes"));
        }
        builder.add(globset::Glob::new(pattern).map_err(|e| invalid(e.to_string()))?);
    }
    builder.build().map_err(|e| invalid(e.to_string()))
}
fn language(value: &str) -> Option<&'static str> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "rs" | "rust" => "rust",
        "js" | "mjs" | "cjs" | "javascript" => "javascript",
        "ts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        "py" | "python" => "python",
        "md" | "markdown" => "markdown",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        _ => return None,
    })
}
