//! Persistent workspace folder-trust MVP.
//!
//! Folder trust is **not** operator permission approval (`docs/permissions.md`).
//! Permissions decide whether the coordinator may run a tool; folder trust
//! decides whether repository-local / path-qualified executables may be spawned
//! for a workspace path.
//!
//! Decisions persist under `.agent-harness/folder-trust.json` (path keys +
//! allow/deny only — no secrets). Trust metadata is not written into session
//! events as raw secrets.

mod store;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use store::{
    FolderTrustError, FolderTrustStore, FolderTrustSummary, FOLDER_TRUST_RELATIVE_PATH,
};

/// Operator decision for a workspace folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderTrustDecision {
    Allow,
    Deny,
}

impl FolderTrustDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub const fn is_allow(self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Result of the pre-spawn gate for repository-local executables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalExecutableGate {
    /// Not a path-qualified / repo-local executable — gate does not apply.
    NotApplicable,
    /// Trust allows spawn of this local executable.
    Allowed,
    /// Trust missing or deny — must block before spawn (no side effects).
    Denied { reason: String },
}

impl LocalExecutableGate {
    pub const fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }

    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed | Self::NotApplicable)
    }
}

/// True when `executable` is path-qualified (repo-local / absolute / relative path).
///
/// PATH-only bare names (`git`, `cargo`) are not gated by folder trust.
pub fn is_repository_local_executable(executable: &str) -> bool {
    let trimmed = executable.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.contains('/')
        || trimmed.contains('\\')
}

/// Gate spawn of a repository-local executable under folder trust.
///
/// - Bare PATH commands → [`LocalExecutableGate::NotApplicable`]
/// - Path-qualified + `Some(Allow)` → [`LocalExecutableGate::Allowed`]
/// - Path-qualified + missing or `Deny` → [`LocalExecutableGate::Denied`]
pub fn gate_repository_local_executable(
    executable: &str,
    workspace_root: &Path,
    trust: Option<FolderTrustDecision>,
) -> LocalExecutableGate {
    if !is_repository_local_executable(executable) {
        return LocalExecutableGate::NotApplicable;
    }

    match trust {
        Some(FolderTrustDecision::Allow) => LocalExecutableGate::Allowed,
        Some(FolderTrustDecision::Deny) => LocalExecutableGate::Denied {
            reason: format!(
                "folder trust denies repository-local executable `{}` for workspace {} \
                 (operator permission allow is not folder trust; trust this folder or use a PATH binary)",
                executable,
                workspace_root.display()
            ),
        },
        None => LocalExecutableGate::Denied {
            reason: format!(
                "folder trust missing for workspace {}; refusing repository-local executable `{}` \
                 before spawn (set folder trust allow for this workspace first)",
                workspace_root.display(),
                executable
            ),
        },
    }
}

/// Load trust for `workspace_root` from the default store and gate `executable`.
pub fn gate_repository_local_executable_from_store(
    executable: &str,
    workspace_root: &Path,
) -> Result<LocalExecutableGate, FolderTrustError> {
    if !is_repository_local_executable(executable) {
        return Ok(LocalExecutableGate::NotApplicable);
    }
    let store = FolderTrustStore::for_workspace(workspace_root);
    let trust = store.get(workspace_root)?;
    Ok(gate_repository_local_executable(
        executable,
        workspace_root,
        trust,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use std::fs;

    #[test]
    fn trusted_workspace_allows_repo_local_executable() {
        // arrange
        // act
        // assert
        // Given: Allow decision for workspace
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let store = FolderTrustStore::for_workspace(&workspace);
        store
            .set(&workspace, FolderTrustDecision::Allow)
            .unwrap_or_abort();

        // When
        let trust = store.get(&workspace).unwrap_or_abort();
        let gate = gate_repository_local_executable("./scripts/tool.sh", &workspace, trust);

        // Then
        assert_eq!(gate, LocalExecutableGate::Allowed);
        assert!(gate.is_allowed());
    }

    #[test]
    fn untrusted_workspace_denies_repo_local_executable_before_spawn() {
        // arrange
        // act
        // assert
        // Given: no trust entry
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();

        // When: gate without side effects (no spawn)
        let gate = gate_repository_local_executable("./bin/helper", &workspace, None);

        // Then: denied with explicit reason
        assert!(gate.is_denied());
        match gate {
            LocalExecutableGate::Denied { reason } => {
                assert!(reason.contains("folder trust missing"));
                assert!(reason.contains("./bin/helper"));
            }
            other => panic!("expected Denied, got {other:?}"),
        }

        let denied = gate_repository_local_executable(
            "/abs/ws/tool",
            &workspace,
            Some(FolderTrustDecision::Deny),
        );
        assert!(denied.is_denied());
    }

    #[test]
    fn bare_path_commands_are_not_gated_by_folder_trust() {
        // arrange
        // act
        // assert
        let workspace = Path::new("/tmp/ws");
        assert_eq!(
            gate_repository_local_executable("git", workspace, None),
            LocalExecutableGate::NotApplicable
        );
        assert_eq!(
            gate_repository_local_executable("cargo", workspace, None),
            LocalExecutableGate::NotApplicable
        );
        assert!(!is_repository_local_executable("ls"));
        assert!(is_repository_local_executable("./ls"));
        assert!(is_repository_local_executable("tools/run"));
    }

    #[test]
    fn folder_trust_persists_allow_and_deny() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("project");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let store = FolderTrustStore::for_workspace(&workspace);

        store
            .set(&workspace, FolderTrustDecision::Allow)
            .unwrap_or_abort();
        assert_eq!(
            store.get(&workspace).unwrap_or_abort(),
            Some(FolderTrustDecision::Allow)
        );

        let reopened = FolderTrustStore::for_workspace(&workspace);
        assert_eq!(
            reopened.get(&workspace).unwrap_or_abort(),
            Some(FolderTrustDecision::Allow)
        );

        reopened
            .set(&workspace, FolderTrustDecision::Deny)
            .unwrap_or_abort();
        assert_eq!(
            reopened.get(&workspace).unwrap_or_abort(),
            Some(FolderTrustDecision::Deny)
        );

        let raw = fs::read_to_string(store.path()).unwrap_or_abort();
        assert!(raw.contains("\"decision\""));
        assert!(!raw.to_lowercase().contains("token"));
        assert!(!raw.to_lowercase().contains("api_key"));
        assert!(!raw.to_lowercase().contains("password"));
    }

    #[test]
    fn summarize_is_redacted_and_workspace_scoped() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("proj");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let store = FolderTrustStore::for_workspace(&workspace);
        store
            .set(&workspace, FolderTrustDecision::Allow)
            .unwrap_or_abort();

        let summary = store.summarize(&workspace).unwrap_or_abort();
        assert_eq!(summary.decision, Some(FolderTrustDecision::Allow));
        assert_eq!(summary.entry_count, 1);
        assert!(summary.store_path.contains("folder-trust.json"));
        let json = serde_json::to_string(&summary).unwrap_or_abort();
        assert!(!json.contains("BEGIN "));
        assert!(!json.contains("sk-"));
    }

    #[test]
    fn gate_from_store_denies_when_missing_and_allows_when_trusted() {
        // arrange
        // act
        // assert
        let temp = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp.path().join("ws");
        fs::create_dir_all(&workspace).unwrap_or_abort();

        let missing =
            gate_repository_local_executable_from_store("./x", &workspace).unwrap_or_abort();
        assert!(missing.is_denied());

        FolderTrustStore::for_workspace(&workspace)
            .set(&workspace, FolderTrustDecision::Allow)
            .unwrap_or_abort();
        let allowed =
            gate_repository_local_executable_from_store("./x", &workspace).unwrap_or_abort();
        assert_eq!(allowed, LocalExecutableGate::Allowed);
    }
}
