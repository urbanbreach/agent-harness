use crate::UnwrapOrAbort;
use std::path::{Path, PathBuf};

use harness_core::tool::{ToolContext, ToolError};
use harness_core::ToolResultExt;

use crate::fs_walk::normalize_relative_path;

pub(crate) fn file_uri_from_path(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    reqwest::Url::from_file_path(&canonical)
        .unwrap_or_abort()
        .to_string()
}

pub(crate) fn file_path_from_uri(uri: &str) -> Option<PathBuf> {
    reqwest::Url::parse(uri).ok()?.to_file_path().ok()
}

pub(crate) fn workspace_relative_path_from_file_uri(
    workspace_root: &Path,
    uri: &str,
) -> Result<String, ToolError> {
    let url = reqwest::Url::parse(uri)
        .tool_err("invalid workspace edit uri")?;
    let path = url.to_file_path().map_err(|_| {
        ToolError::Execution(format!(
            "workspace edit uri does not map to a local filesystem path: {uri}"
        ))
    })?;
    let normalized = normalize_workspace_target_path(workspace_root, &path)?;
    normalized
        .strip_prefix(workspace_root)
        .map(normalize_relative_path)
        .map_err(|_| ToolError::PathEscapesWorkspace {
            workspace_root: workspace_root.display().to_string(),
            path: normalized.display().to_string(),
        })
}

pub(crate) fn resolve_existing_path(ctx: &ToolContext, input: &str) -> Result<PathBuf, ToolError> {
    let workspace = canonical_workspace_root(ctx)?;
    let candidate = normalize_workspace_target_path(&workspace, Path::new(input))?;
    let canonical = if candidate == workspace {
        workspace.clone()
    } else {
        candidate
            .canonicalize()
            .tool_err("failed to resolve path")?
    };
    ensure_within_workspace_path(&workspace, &canonical)?;
    Ok(canonical)
}

pub(crate) fn canonical_workspace_root(ctx: &ToolContext) -> Result<PathBuf, ToolError> {
    ctx.workspace_root
        .canonicalize()
        .tool_err("failed to resolve workspace root")
}

pub(crate) fn ensure_within_workspace_path(
    workspace: &Path,
    candidate: &Path,
) -> Result<(), ToolError> {
    if candidate.starts_with(workspace) {
        Ok(())
    } else {
        Err(ToolError::PathEscapesWorkspace {
            workspace_root: workspace.display().to_string(),
            path: candidate.display().to_string(),
        })
    }
}

pub(crate) fn normalize_workspace_target_path(
    workspace: &Path,
    input: &Path,
) -> Result<PathBuf, ToolError> {
    let relative = if input.is_absolute() {
        input
            .strip_prefix(workspace)
            .map_err(|_| ToolError::PathEscapesWorkspace {
                workspace_root: workspace.display().to_string(),
                path: input.display().to_string(),
            })?
    } else {
        input
    };

    let mut normalized = workspace.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => normalized.push(segment),
            std::path::Component::ParentDir => {
                if normalized == workspace {
                    return Err(ToolError::PathEscapesWorkspace {
                        workspace_root: workspace.display().to_string(),
                        path: input.display().to_string(),
                    });
                }
                normalized.pop();
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(ToolError::InvalidArguments(
                    "path must be workspace-relative or inside the workspace".to_string(),
                ));
            }
        }
    }
    Ok(normalized)
}
