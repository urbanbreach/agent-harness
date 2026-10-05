use serde::{Deserialize, Serialize};
pub mod ruleset;
pub use ruleset::{PermissionAction, PermissionPolicy, PermissionRule, PermissionRuleset};

pub fn external_path_prefix_covers(prefix: &str, requested: &str) -> bool {
    use std::path::{Component, Path};
    let prefix = Path::new(prefix);
    let requested = Path::new(requested);
    prefix.is_absolute()
        && requested.is_absolute()
        && !prefix
            .components()
            .chain(requested.components())
            .any(|c| matches!(c, Component::ParentDir))
        && requested.starts_with(prefix)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    EditFs,
    Shell,
    Network,
    Question,
    Task,
    Eval,
    WebFetch,
    WebSearch,
    CodeSearch,
    Lsp,
    Read,
    ExternalDirectory,
    DoomLoop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionGrantScope {
    #[default]
    Run,
    Session,
    Workspace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionToolSelector {
    pub effective_tool_id: String,
    pub canonical_tool_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "selector", rename_all = "snake_case")]
pub enum PermissionGrantMatcher {
    RequestDigest {
        request_digest: String,
    },
    ShellCommand {
        command_digest: String,
        request_digest: String,
        #[serde(default, skip_serializing)]
        patterns: Vec<String>,
        #[serde(default)]
        always_patterns: Vec<String>,
    },
    WorkspacePath {
        path: String,
        request_digest: String,
    },
    ExternalPath {
        path_prefix: String,
        request_digest: String,
    },
}

impl PermissionGrantMatcher {
    pub(crate) fn digest(&self) -> &str {
        match self {
            Self::RequestDigest { request_digest }
            | Self::WorkspacePath { request_digest, .. }
            | Self::ExternalPath { request_digest, .. }
            | Self::ShellCommand { request_digest, .. } => request_digest,
        }
    }
    pub(crate) fn matches(&self, request: &Self) -> bool {
        if self.digest() == request.digest() {
            return true;
        }
        match (self, request) {
            (Self::WorkspacePath { path: a, .. }, Self::WorkspacePath { path: b, .. }) => a == b,
            (
                Self::ExternalPath {
                    path_prefix: prefix,
                    ..
                },
                Self::ExternalPath {
                    path_prefix: path, ..
                },
            ) => external_path_prefix_covers(prefix, path),
            (
                Self::ShellCommand {
                    command_digest: a,
                    always_patterns: allowed,
                    ..
                },
                Self::ShellCommand {
                    command_digest: b,
                    always_patterns: requested,
                    ..
                },
            ) => {
                a == b
                    || (!requested.is_empty()
                        && requested.iter().all(|request| {
                            allowed.iter().any(|pattern| {
                                pattern == request
                                    || pattern
                                        .strip_suffix('*')
                                        .is_some_and(|prefix| request.starts_with(prefix))
                            })
                        }))
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrantRequest {
    pub kind: PermissionKind,
    pub tool: PermissionToolSelector,
    pub matcher: PermissionGrantMatcher,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub grant_id: String,
    pub permission_id: String,
    pub scope: PermissionGrantScope,
    pub expires_at: Option<String>,
    pub kind: PermissionKind,
    pub tool: PermissionToolSelector,
    pub matcher: PermissionGrantMatcher,
}
