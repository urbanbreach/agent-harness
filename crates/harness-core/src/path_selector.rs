use std::path::{Component, Path};

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

    #[test]
    fn workspace_selector_paths_normalize_relative_components() {
        // arrange
        // act
        // assert
        assert_eq!(
            normalize_workspace_relative_path(Path::new("./src/./lib.rs")),
            Some("src/lib.rs".to_string())
        );
        assert_eq!(normalize_workspace_relative_path(Path::new(".")), None);
        assert_eq!(normalize_workspace_relative_path(Path::new("../src")), None);
    }

    #[test]
    fn workspace_selector_paths_accept_absolute_inside_workspace_only_when_requested() {
        // arrange
        // act
        // assert
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
}
