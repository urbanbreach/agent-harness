use std::path::PathBuf;

use harness_core::workspace::WorkspaceEnvironment;

pub(super) fn workspace_context_labels(environment: &WorkspaceEnvironment) -> Vec<String> {
    let full = directory_branch_label(environment, false);
    let short = directory_branch_label(environment, true);
    if full == short {
        vec![full]
    } else {
        vec![full, short]
    }
}

pub(super) fn directory_branch_label(environment: &WorkspaceEnvironment, short: bool) -> String {
    let path = if short {
        environment
            .working_directory
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| home_shortened_path(&environment.working_directory))
    } else {
        home_shortened_path(&environment.working_directory)
    };

    match environment.git_branch.as_deref() {
        Some(branch) if !branch.trim().is_empty() => format!("{path}:{branch}"),
        _ => path,
    }
}

fn home_shortened_path(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home.as_deref().filter(|home| !home.as_os_str().is_empty()) {
        if path == home {
            return "~".to_string();
        }
        if let Ok(stripped) = path.strip_prefix(home) {
            return format!("~/{}", stripped.display());
        }
    }

    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(path: &str, branch: Option<&str>) -> WorkspaceEnvironment {
        WorkspaceEnvironment {
            working_directory: PathBuf::from(path),
            workspace_root: PathBuf::from(path),
            is_git_repository: branch.is_some(),
            git_branch: branch.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn workspace_context_labels_include_full_and_short_when_distinct() {
        // arrange
        // act
        // assert
        let labels =
            workspace_context_labels(&environment("/workspace/agent-harness", Some("dev")));

        assert_eq!(
            labels,
            vec![
                "/workspace/agent-harness:dev".to_string(),
                "agent-harness:dev".to_string(),
            ]
        );
    }

    #[test]
    fn workspace_context_labels_deduplicate_equal_full_and_short_labels() {
        // arrange
        // act
        // assert
        let labels = workspace_context_labels(&environment("/", None));

        assert_eq!(labels, vec!["/".to_string()]);
    }

    #[test]
    fn directory_branch_label_ignores_blank_branch() {
        // arrange
        // act
        // assert
        let label =
            directory_branch_label(&environment("/workspace/agent-harness", Some("   ")), false);

        assert_eq!(label, "/workspace/agent-harness");
    }
}
