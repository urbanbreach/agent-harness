use std::path::{Component, Path, PathBuf};

use crate::tool::{normalize_workspace_target_path, ToolError};

pub(crate) struct EffectiveWorkspaceTarget {
    pub(crate) requested: Option<String>,
    pub(crate) relative: Option<String>,
    pub(crate) target: PathBuf,
}

/// Resolve runtime targets, including missing creation leaves. `relative: None`
/// identifies an external target; resolution failures never mean no selector.
pub(crate) fn effective_workspace_target(
    workspace: &Path,
    input: &Path,
) -> Result<EffectiveWorkspaceTarget, ToolError> {
    if input.as_os_str().is_empty() {
        return Err(ToolError::InvalidArguments("empty file path".into()));
    }
    let workspace =
        workspace
            .canonicalize()
            .map_err(|source| ToolError::WorkspaceRootUnavailable {
                path: workspace.display().to_string(),
                source,
            })?;
    if !workspace.is_dir() {
        return Err(ToolError::InvalidArguments(
            "workspace root is not a directory".into(),
        ));
    }
    let candidate = match normalize_workspace_target_path(&workspace, input) {
        Ok(path) => path,
        Err(ToolError::PathEscapesWorkspace { path, .. }) => {
            let absolute = workspace.join(path);
            // Reuse the execution normalizer, with the filesystem root as the boundary.
            let root = absolute
                .ancestors()
                .last()
                .ok_or_else(|| ToolError::InvalidArguments("path has no root".into()))?;
            normalize_workspace_target_path(root, &absolute)?
        }
        Err(error) => return Err(error),
    };
    let mut ancestor = candidate.as_path();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or_else(|| ToolError::PathResolution {
                    path: candidate.display().to_string(),
                    source: error,
                })?;
            }
            Err(source) => {
                return Err(ToolError::PathResolution {
                    path: candidate.display().to_string(),
                    source,
                })
            }
        }
    }
    let canonical = ancestor
        .canonicalize()
        .map_err(|source| ToolError::PathResolution {
            path: ancestor.display().to_string(),
            source,
        })?;
    let suffix = candidate
        .strip_prefix(ancestor)
        .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
    if !suffix.as_os_str().is_empty() && !canonical.is_dir() {
        return Err(ToolError::InvalidArguments(
            "file path ancestor is not a directory".into(),
        ));
    }
    let target = if suffix.as_os_str().is_empty() {
        canonical
    } else {
        canonical.join(suffix)
    };
    if target.to_str().is_none() {
        return Err(ToolError::InvalidArguments(
            "file target is not valid UTF-8".into(),
        ));
    }
    let relative = |path: &Path| {
        path.strip_prefix(&workspace)
            .ok()
            .map(|path| normalize_relative_components(path).unwrap_or_else(|| ".".into()))
    };
    Ok(EffectiveWorkspaceTarget {
        requested: relative(&candidate),
        relative: relative(&target),
        target,
    })
}

pub(crate) fn normalize_workspace_relative_path(path: &Path) -> Option<String> {
    if path.is_absolute() {
        return None;
    }
    normalize_relative_components(path)
}

pub(crate) fn workspace_relative_path_from_maybe_absolute(
    workspace_root: &Path,
    path: &Path,
) -> Option<String> {
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace_root).ok()?
    } else {
        path
    };
    normalize_relative_components(relative)
}

fn normalize_relative_components(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;

    #[test]
    fn effective_target_requires_an_existing_workspace_directory() {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let file = temp.path().join("file");
        std::fs::write(&file, "file").unwrap_or_abort();
        for root in [file, temp.path().join("missing")] {
            assert!(effective_workspace_target(&root, Path::new("new/leaf")).is_err());
        }
    }

    #[test]
    fn workspace_selector_paths_normalize_relative_components() {
        assert_eq!(
            normalize_workspace_relative_path(Path::new("./src/./lib.rs")),
            Some("src/lib.rs".to_string())
        );
        assert_eq!(normalize_workspace_relative_path(Path::new(".")), None);
        assert_eq!(normalize_workspace_relative_path(Path::new("../src")), None);
    }

    #[test]
    fn workspace_selector_paths_accept_absolute_inside_workspace_only_when_requested() {
        let workspace = Path::new("/workspace/project");

        assert_eq!(
            workspace_relative_path_from_maybe_absolute(
                workspace,
                Path::new("/workspace/project/src/main.rs")
            ),
            Some("src/main.rs".to_string())
        );
        assert_eq!(
            workspace_relative_path_from_maybe_absolute(
                workspace,
                Path::new("/workspace/other/src/main.rs")
            ),
            None
        );
        assert_eq!(
            normalize_workspace_relative_path(Path::new("/workspace/project/src/main.rs")),
            None
        );
    }

    #[test]
    fn workspace_selector_rejects_parent_traversal_inside_absolute_path() {
        // arrange — a workspace prefix that an escaping path could match
        let workspace = Path::new("/workspace/project");

        // act — resolution of paths that escape the prefix
        // assert — prefix matches must not smuggle `..` components through
        assert_eq!(
            workspace_relative_path_from_maybe_absolute(
                workspace,
                Path::new("/workspace/project/../evil/x")
            ),
            None
        );
        assert_eq!(
            normalize_workspace_relative_path(Path::new("src/../../x")),
            None
        );
    }
}
