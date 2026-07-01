use harness_core::tool::{ToolContext, ToolError};

use crate::hashline_apply::resolve_workspace_target_path;

use super::parser::{derive_update, Hunk};

pub(super) enum PreparedHunk {
    Add {
        path: String,
        contents: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        source: String,
        content: String,
    },
}

impl PreparedHunk {
    pub(super) fn path(&self) -> &str {
        match self {
            Self::Add { path, .. } | Self::Delete { path } | Self::Update { path, .. } => path,
        }
    }

    pub(super) fn kind(&self) -> &'static str {
        match self {
            Self::Add { .. } => "add",
            Self::Delete { .. } => "delete",
            Self::Update { .. } => "update",
        }
    }
}

pub(super) fn prepare_hunks(
    ctx: &ToolContext,
    hunks: &[Hunk],
) -> Result<Vec<PreparedHunk>, ToolError> {
    if hunks.iter().any(|hunk| {
        matches!(
            hunk,
            Hunk::Update {
                move_path: Some(_),
                ..
            }
        )
    }) {
        return Err(ToolError::Execution(
            "apply_patch moves are not supported yet".to_string(),
        ));
    }

    hunks.iter().map(|hunk| prepare_hunk(ctx, hunk)).collect()
}

fn prepare_hunk(ctx: &ToolContext, hunk: &Hunk) -> Result<PreparedHunk, ToolError> {
    match hunk {
        Hunk::Add { path, contents } => {
            resolve_workspace_target_path(ctx, path)?;
            Ok(PreparedHunk::Add {
                path: path.clone(),
                contents: contents.clone(),
            })
        }
        Hunk::Delete { path } => {
            let target = resolve_workspace_target_path(ctx, path)?;
            let metadata = std::fs::metadata(&target).map_err(|_| preflight_failure(path))?;
            if !metadata.is_file() {
                return Err(preflight_failure(path));
            }
            Ok(PreparedHunk::Delete { path: path.clone() })
        }
        Hunk::Update { path, chunks, .. } => {
            let target = resolve_workspace_target_path(ctx, path)?;
            let source = std::fs::read_to_string(&target).map_err(|_| preflight_failure(path))?;
            let content =
                derive_update(path, chunks, &source).map_err(|_| preflight_failure(path))?;
            Ok(PreparedHunk::Update {
                path: path.clone(),
                source,
                content,
            })
        }
    }
}

fn preflight_failure(path: &str) -> ToolError {
    ToolError::Execution(format!("Unable to apply patch at {path}"))
}
