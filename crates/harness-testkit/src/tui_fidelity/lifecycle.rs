use std::collections::BTreeSet;
use std::path::{Component, Path};

use super::error::{CleanupError, ExitCodeError, ScenarioError};
use super::types::CleanupExpectation;

pub(super) fn validate_exit_code(code: i32) -> Result<(), ScenarioError> {
    if code < 0 {
        Err(ScenarioError::InvalidExitCode(ExitCodeError::Negative(
            code,
        )))
    } else if code > 255 {
        Err(ScenarioError::InvalidExitCode(ExitCodeError::TooLarge(
            code,
        )))
    } else {
        Ok(())
    }
}

pub(super) fn validate_cleanup(cleanup: &CleanupExpectation) -> Result<(), ScenarioError> {
    if !cleanup.restore_workspace {
        return Err(ScenarioError::InvalidCleanup(
            CleanupError::WorkspaceNotRestored,
        ));
    }
    if !cleanup.preserve_evidence {
        return Err(ScenarioError::InvalidCleanup(
            CleanupError::EvidenceNotPreserved,
        ));
    }
    if cleanup.temporary_paths.is_empty() {
        return Err(ScenarioError::InvalidCleanup(
            CleanupError::NoTemporaryPaths,
        ));
    }
    let mut paths = BTreeSet::new();
    for path in &cleanup.temporary_paths {
        let parsed = Path::new(path);
        if path.is_empty()
            || parsed.is_absolute()
            || parsed.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(ScenarioError::InvalidCleanup(CleanupError::InvalidPath));
        }
        if !paths.insert(path) {
            return Err(ScenarioError::InvalidCleanup(CleanupError::DuplicatePath));
        }
    }
    Ok(())
}
