use std::path::{Path, PathBuf};

use harness_core::tool::{ToolContext, ToolError};
use walkdir::{DirEntry, WalkDir};

use crate::hashline_apply::resolve_workspace_target_path;

pub(crate) const SKIPPED_DIR_NAMES: &[&str] = &[".git", "target"];
pub(crate) const SKIPPED_RELATIVE_DIRS: &[&str] = &[".agent-harness/sessions"];
pub(crate) const SKIPPED_WORKSPACE_DIRS: &[&str] = &[".git", "target", ".agent-harness/sessions"];
pub(crate) const DEFAULT_SEARCH_PATH: &str = ".";

pub(crate) fn resolve_search_base(
    ctx: &ToolContext,
    path: Option<&str>,
) -> Result<(PathBuf, PathBuf, String), ToolError> {
    let base_path = path.unwrap_or(DEFAULT_SEARCH_PATH);
    let workspace_root = ctx.resolve_workspace_path(Path::new("."))?;
    let resolved_path = resolve_workspace_target_path(ctx, base_path)?;
    let display_path = workspace_relative_display(&workspace_root, &resolved_path)?;

    Ok((workspace_root, resolved_path, display_path))
}

pub(crate) fn should_skip_entry(workspace_root: &Path, entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }

    let path = entry.path();
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if entry.file_type().is_dir() && SKIPPED_DIR_NAMES.contains(&name) {
        return true;
    }

    let Ok(relative) = path.strip_prefix(workspace_root) else {
        return false;
    };
    let relative = normalize_relative_path(relative);

    SKIPPED_RELATIVE_DIRS
        .iter()
        .any(|prefix| relative == *prefix || relative.starts_with(&format!("{prefix}/")))
}

pub(crate) fn normalize_relative_path(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        return ".".to_string();
    }

    path.iter()
        .map(|segment| segment.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn normalize_base_relative_path(base: &Path, path: &Path) -> String {
    normalize_relative_path(path.strip_prefix(base).unwrap_or(path))
}

pub(crate) fn normalize_workspace_relative_entry(
    workspace_root: &Path,
    path: &Path,
) -> Result<String, ToolError> {
    let relative = path.strip_prefix(workspace_root).map_err(|_| {
        ToolError::Execution(format!(
            "failed to compute workspace-relative path for {}",
            path.display()
        ))
    })?;
    Ok(normalize_relative_path(relative))
}

pub(crate) fn workspace_relative_display(
    workspace_root: &Path,
    resolved_path: &Path,
) -> Result<String, ToolError> {
    let relative = resolved_path.strip_prefix(workspace_root).map_err(|_| {
        ToolError::PathEscapesWorkspace {
            workspace_root: workspace_root.display().to_string(),
            path: resolved_path.display().to_string(),
        }
    })?;
    Ok(normalize_relative_path(relative))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceFile {
    pub(crate) relative_path: String,
    pub(crate) file_name: String,
    pub(crate) path: PathBuf,
}

pub(crate) fn collect_workspace_files(
    workspace_root: &Path,
    search_path: &Path,
) -> Result<Vec<WorkspaceFile>, ToolError> {
    let mut files = Vec::new();
    for entry in WalkDir::new(search_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !should_skip_entry(workspace_root, entry))
    {
        let entry = entry.map_err(|err| {
            ToolError::Execution(format!("failed to traverse directory tree: {err}"))
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        files.push(workspace_file_from_entry(workspace_root, entry)?);
    }
    Ok(files)
}

pub(crate) fn workspace_file_from_path(
    workspace_root: &Path,
    path: &Path,
) -> Result<WorkspaceFile, ToolError> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    workspace_file(workspace_root, path.to_path_buf(), file_name)
}

fn workspace_file_from_entry(
    workspace_root: &Path,
    entry: DirEntry,
) -> Result<WorkspaceFile, ToolError> {
    let file_name = entry.file_name().to_string_lossy().to_string();
    let path = entry.into_path();
    workspace_file(workspace_root, path, file_name)
}

fn workspace_file(
    workspace_root: &Path,
    path: PathBuf,
    file_name: String,
) -> Result<WorkspaceFile, ToolError> {
    Ok(WorkspaceFile {
        relative_path: normalize_workspace_relative_entry(workspace_root, &path)?,
        file_name,
        path,
    })
}
