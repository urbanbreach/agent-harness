//! Workspace-scoped path validation for plugin package roots.

use std::path::{Path, PathBuf};

use super::plugin::PluginLifecycleError;

pub(super) fn resolve_under_workspace(
    workspace_root: &Path,
    package_root: &Path,
) -> Result<PathBuf, PluginLifecycleError> {
    let canonical_workspace = workspace_root.canonicalize().map_err(|err| {
        PluginLifecycleError::WorkspaceRootUnavailable {
            path: workspace_root.display().to_string(),
            message: err.to_string(),
        }
    })?;

    let candidate = if package_root.is_absolute() {
        package_root.to_path_buf()
    } else {
        canonical_workspace.join(package_root)
    };

    // Lexical escape check before canonicalize so missing/malicious paths fail closed.
    if path_lexically_escapes(&canonical_workspace, &candidate) {
        return Err(PluginLifecycleError::PathEscapesWorkspace {
            workspace_root: canonical_workspace.display().to_string(),
            path: candidate.display().to_string(),
        });
    }

    let canonical_package = candidate.canonicalize().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            PluginLifecycleError::PackageRootNotDirectory {
                path: candidate.display().to_string(),
            }
        } else {
            PluginLifecycleError::ManifestRead {
                path: candidate.display().to_string(),
                message: err.to_string(),
            }
        }
    })?;

    if !canonical_package.starts_with(&canonical_workspace) {
        return Err(PluginLifecycleError::PathEscapesWorkspace {
            workspace_root: canonical_workspace.display().to_string(),
            path: canonical_package.display().to_string(),
        });
    }

    Ok(canonical_package)
}

fn path_lexically_escapes(workspace_root: &Path, candidate: &Path) -> bool {
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return true;
                }
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.is_absolute() {
        return !(normalized == workspace_root || normalized.starts_with(workspace_root));
    }
    false
}
