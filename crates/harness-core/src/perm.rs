// allow: SIZE_OK — permission system (capability mapping + policy resolution + ask timeout + deny rules)
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{
    CategoryPermissions, HarnessConfig, PermissionMode, PermissionRuleSet, PermissionSelector,
    PermissionSelectorRule,
};
use crate::tool::{canonical_tool_id_for, ToolCapability};

pub mod shell;

const DEFAULT_ASK_TIMEOUT_MS: u64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    EditFs,
    Shell,
    Network,
    Question,
    Task,
    WebFetch,
    WebSearch,
    CodeSearch,
    Lsp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionGrantScope {
    #[default]
    Run,
    Session,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionToolSelector {
    pub effective_tool_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
}

impl PermissionToolSelector {
    pub fn matches(&self, request: &Self) -> bool {
        match (&self.canonical_tool_id, &request.canonical_tool_id) {
            (Some(left), Some(right)) => left == right,
            _ => self.effective_tool_id == request.effective_tool_id,
        }
    }
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        always_patterns: Vec<String>,
    },
    WorkspacePath {
        path: String,
        request_digest: String,
    },
}

impl PermissionGrantMatcher {
    pub fn matches(&self, request: &Self) -> bool {
        match (self, request) {
            (
                Self::ShellCommand {
                    command_digest: granted,
                    request_digest: granted_request,
                    always_patterns: granted_always_patterns,
                    ..
                },
                Self::ShellCommand {
                    command_digest: requested,
                    request_digest,
                    always_patterns: requested_always_patterns,
                    ..
                },
            ) => {
                granted == requested
                    || granted_request == request_digest
                    || shell_always_patterns_authorize(
                        granted_always_patterns,
                        requested_always_patterns,
                    )
            }
            (
                Self::WorkspacePath {
                    path: granted,
                    request_digest: granted_request,
                },
                Self::WorkspacePath {
                    path: requested,
                    request_digest,
                },
            ) => granted == requested || granted_request == request_digest,
            (
                Self::RequestDigest {
                    request_digest: granted,
                },
                candidate,
            ) => granted == candidate.request_digest(),
            (candidate, Self::RequestDigest { request_digest }) => {
                candidate.request_digest() == request_digest
            }
            _ => self.request_digest() == request.request_digest(),
        }
    }

    pub fn request_digest(&self) -> &str {
        match self {
            Self::RequestDigest { request_digest }
            | Self::ShellCommand { request_digest, .. }
            | Self::WorkspacePath { request_digest, .. } => request_digest,
        }
    }
}

fn shell_always_patterns_authorize(granted: &[String], requested: &[String]) -> bool {
    !requested.is_empty()
        && requested.iter().all(|requested_pattern| {
            granted.iter().any(|granted_pattern| {
                shell::shell_permission_pattern_matches(granted_pattern, requested_pattern)
            })
        })
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub kind: PermissionKind,
    pub tool: PermissionToolSelector,
    pub matcher: PermissionGrantMatcher,
}

impl PermissionGrant {
    pub fn matches(&self, request: &PermissionGrantRequest) -> bool {
        self.expires_at.is_none()
            && self.kind == request.kind
            && self.tool.matches(&request.tool)
            && self.matcher.matches(&request.matcher)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionGrantSet {
    grants: Vec<PermissionGrant>,
}

impl PermissionGrantSet {
    pub fn from_grants(grants: impl IntoIterator<Item = PermissionGrant>) -> Self {
        let mut set = Self::default();
        for grant in grants {
            set.record(grant);
        }
        set
    }

    pub fn record(&mut self, grant: PermissionGrant) {
        self.grants
            .retain(|existing| existing.grant_id != grant.grant_id);
        self.grants.push(grant);
    }

    pub fn authorizes(&self, request: &PermissionGrantRequest) -> bool {
        self.grants.iter().any(|grant| grant.matches(request))
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }
}

impl PermissionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EditFs => "edit_fs",
            Self::Shell => "shell",
            Self::Network => "network",
            Self::Question => "question",
            Self::Task => "task",
            Self::WebFetch => "webfetch",
            Self::WebSearch => "websearch",
            Self::CodeSearch => "codesearch",
            Self::Lsp => "lsp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    Ask {
        timeout_ms: u64,
        default_decision: PermissionDecision,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRuleRequest {
    ShellCommand { pattern: String },
    WorkspacePath(String),
    TaskAgent(String),
}

#[derive(Debug, Clone)]
pub struct PermissionPolicy {
    defaults: DefaultPermissionModes,
    default_rules: PermissionRuleSet,
    profile_overrides: BTreeMap<String, CategoryPermissions>,
    ask_timeout_ms: u64,
}

#[derive(Debug, Clone)]
struct DefaultPermissionModes {
    edit: PermissionMode,
    shell: PermissionMode,
    network: PermissionMode,
    question: PermissionMode,
    task: PermissionMode,
    webfetch: PermissionMode,
    websearch: PermissionMode,
    codesearch: PermissionMode,
    lsp: PermissionMode,
}

impl DefaultPermissionModes {
    fn from_config(config: &HarnessConfig) -> Self {
        let legacy_network = config.permissions.network.clone();

        Self {
            edit: config.permissions.edit.clone(),
            shell: config.permissions.shell.clone(),
            network: legacy_network.clone(),
            question: config
                .permissions
                .question
                .clone()
                .unwrap_or(PermissionMode::Ask),
            task: config
                .permissions
                .task
                .clone()
                .unwrap_or(PermissionMode::Allow),
            webfetch: config
                .permissions
                .webfetch
                .clone()
                .unwrap_or_else(|| legacy_network.clone()),
            websearch: config
                .permissions
                .websearch
                .clone()
                .unwrap_or_else(|| legacy_network.clone()),
            codesearch: config
                .permissions
                .codesearch
                .clone()
                .unwrap_or_else(|| legacy_network.clone()),
            lsp: config
                .permissions
                .lsp
                .clone()
                .unwrap_or(PermissionMode::Allow),
        }
    }

    fn from_legacy_defaults(
        edit: PermissionMode,
        shell: PermissionMode,
        network: PermissionMode,
    ) -> Self {
        Self {
            edit,
            shell,
            network: network.clone(),
            question: PermissionMode::Ask,
            task: PermissionMode::Allow,
            webfetch: network.clone(),
            websearch: network.clone(),
            codesearch: network,
            lsp: PermissionMode::Allow,
        }
    }
}

impl PermissionPolicy {
    pub fn from_config(config: &HarnessConfig) -> Self {
        let profile_overrides = config
            .agents
            .iter()
            .filter_map(|(name, profile)| {
                profile
                    .permissions
                    .clone()
                    .map(|permissions| (name.clone(), permissions))
            })
            .collect();

        Self {
            defaults: DefaultPermissionModes::from_config(config),
            default_rules: config.permissions.rules.clone(),
            profile_overrides,
            ask_timeout_ms: DEFAULT_ASK_TIMEOUT_MS,
        }
    }

    pub fn new(edit: PermissionMode, shell: PermissionMode, network: PermissionMode) -> Self {
        Self {
            defaults: DefaultPermissionModes::from_legacy_defaults(edit, shell, network),
            default_rules: PermissionRuleSet::default(),
            profile_overrides: BTreeMap::new(),
            ask_timeout_ms: DEFAULT_ASK_TIMEOUT_MS,
        }
    }

    pub fn with_ask_timeout_ms(mut self, ask_timeout_ms: u64) -> Self {
        self.ask_timeout_ms = ask_timeout_ms;
        self
    }

    pub fn with_category_override(
        mut self,
        category: impl Into<String>,
        permissions: CategoryPermissions,
    ) -> Self {
        self.profile_overrides.insert(category.into(), permissions);
        self
    }

    pub fn evaluate(&self, profile: Option<&str>, kind: PermissionKind) -> PolicyDecision {
        self.evaluate_request(profile, kind, None)
    }

    pub fn evaluate_request(
        &self,
        profile: Option<&str>,
        kind: PermissionKind,
        selector: Option<&PermissionRuleRequest>,
    ) -> PolicyDecision {
        let effective_mode = profile
            .and_then(|name| self.profile_overrides.get(name))
            .and_then(|permissions| profile_mode_for_request(permissions, kind, selector).cloned())
            .or_else(|| rule_mode_for_kind(&self.default_rules, kind, selector).cloned())
            .unwrap_or_else(|| self.default_mode(kind).clone());

        match effective_mode {
            PermissionMode::Allow => PolicyDecision::Allow,
            PermissionMode::Deny => PolicyDecision::Deny,
            PermissionMode::Ask => PolicyDecision::Ask {
                timeout_ms: self.ask_timeout_ms,
                default_decision: PermissionDecision::Deny,
            },
        }
    }

    fn default_mode(&self, kind: PermissionKind) -> &PermissionMode {
        match kind {
            PermissionKind::EditFs => &self.defaults.edit,
            PermissionKind::Shell => &self.defaults.shell,
            PermissionKind::Network => &self.defaults.network,
            PermissionKind::Question => &self.defaults.question,
            PermissionKind::Task => &self.defaults.task,
            PermissionKind::WebFetch => &self.defaults.webfetch,
            PermissionKind::WebSearch => &self.defaults.websearch,
            PermissionKind::CodeSearch => &self.defaults.codesearch,
            PermissionKind::Lsp => &self.defaults.lsp,
        }
    }
}

fn profile_mode_for_request<'a>(
    permissions: &'a CategoryPermissions,
    kind: PermissionKind,
    selector: Option<&PermissionRuleRequest>,
) -> Option<&'a PermissionMode> {
    rule_mode_for_kind(&permissions.rules, kind, selector)
        .or_else(|| mode_for_kind(permissions, kind))
        .or(permissions.fallback.as_ref())
}

fn rule_mode_for_kind<'a>(
    rules: &'a PermissionRuleSet,
    kind: PermissionKind,
    selector: Option<&PermissionRuleRequest>,
) -> Option<&'a PermissionMode> {
    match kind {
        PermissionKind::Shell => selector_rule_mode(
            &rules.shell,
            selector.and_then(|selector| match selector {
                PermissionRuleRequest::ShellCommand { pattern } => Some(pattern.as_str()),
                PermissionRuleRequest::WorkspacePath(_) | PermissionRuleRequest::TaskAgent(_) => {
                    None
                }
            }),
        ),
        PermissionKind::EditFs => selector_rule_mode(
            &rules.edit,
            selector.and_then(|selector| match selector {
                PermissionRuleRequest::WorkspacePath(path) => Some(path.as_str()),
                PermissionRuleRequest::ShellCommand { .. }
                | PermissionRuleRequest::TaskAgent(_) => None,
            }),
        ),
        PermissionKind::Task => selector_rule_mode(
            &rules.task,
            selector.and_then(|selector| match selector {
                PermissionRuleRequest::TaskAgent(agent) => Some(agent.as_str()),
                PermissionRuleRequest::ShellCommand { .. }
                | PermissionRuleRequest::WorkspacePath(_) => None,
            }),
        ),
        PermissionKind::Network
        | PermissionKind::Question
        | PermissionKind::WebFetch
        | PermissionKind::WebSearch
        | PermissionKind::CodeSearch
        | PermissionKind::Lsp => None,
    }
}

fn selector_rule_mode<'a>(
    rules: &'a [PermissionSelectorRule],
    value: Option<&str>,
) -> Option<&'a PermissionMode> {
    let Some(value) = value else {
        return rules
            .iter()
            .find(|rule| matches!(rule.selector, PermissionSelector::CatchAll))
            .map(|rule| &rule.mode);
    };

    rules
        .iter()
        .rfind(|rule| permission_selector_matches(&rule.selector, value))
        .map(|rule| &rule.mode)
}

fn permission_selector_matches(selector: &PermissionSelector, value: &str) -> bool {
    match selector {
        PermissionSelector::Exact(selector) => selector == value,
        PermissionSelector::Prefix(prefix) => value.starts_with(prefix),
        PermissionSelector::Glob(pattern) => glob_matches(pattern, value),
        PermissionSelector::CatchAll => true,
    }
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let mut remainder = value;
    let mut parts = pattern.split('*').peekable();
    let mut anchored_start = !pattern.starts_with('*');
    while let Some(part) = parts.next() {
        if part.is_empty() {
            anchored_start = false;
            continue;
        }
        if anchored_start {
            let Some(next) = remainder.strip_prefix(part) else {
                return false;
            };
            remainder = next;
            anchored_start = false;
            continue;
        }
        let Some(index) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[index + part.len()..];
        if parts.peek().is_none() && !pattern.ends_with('*') {
            return remainder.is_empty();
        }
    }
    pattern.ends_with('*') || remainder.is_empty()
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self::new(
            PermissionMode::Allow,
            PermissionMode::Allow,
            PermissionMode::Allow,
        )
    }
}

pub fn permission_kind_for_tool(tool_id: &str) -> Option<PermissionKind> {
    let canonical_tool_id = canonical_tool_id_for(tool_id).unwrap_or(tool_id);

    match canonical_tool_id {
        "question" => Some(PermissionKind::Question),
        "plan_enter" | "plan_exit" => Some(PermissionKind::Question),
        "task" | "skill" => Some(PermissionKind::Task),
        "background_cancel" => Some(PermissionKind::Task),
        "todoread" | "todowrite" => Some(PermissionKind::Task),
        "webfetch" => Some(PermissionKind::WebFetch),
        "websearch" => Some(PermissionKind::WebSearch),
        "codesearch" => Some(PermissionKind::CodeSearch),
        "ast_grep_search" => Some(PermissionKind::CodeSearch),
        "ast_grep_replace" => Some(PermissionKind::EditFs),
        "lsp" => Some(PermissionKind::Lsp),
        "lsp.rename" => Some(PermissionKind::EditFs),
        "bash" => Some(PermissionKind::Shell),
        _ if canonical_tool_id.starts_with("edit.") => Some(PermissionKind::EditFs),
        _ if canonical_tool_id.starts_with("shell.") => Some(PermissionKind::Shell),
        _ if canonical_tool_id.starts_with("network.") || canonical_tool_id.starts_with("net.") => {
            Some(PermissionKind::Network)
        }
        _ => None,
    }
}

pub fn permission_kind_for_tool_call(
    tool_id: &str,
    capability: ToolCapability,
) -> Option<PermissionKind> {
    permission_kind_for_tool(tool_id).or_else(|| permission_kind_for_capability(capability))
}

pub fn permission_kind_for_capability(capability: ToolCapability) -> Option<PermissionKind> {
    match capability {
        ToolCapability::EditFs => Some(PermissionKind::EditFs),
        ToolCapability::Shell => Some(PermissionKind::Shell),
        ToolCapability::Network => Some(PermissionKind::Network),
        ToolCapability::SpawnAgent => Some(PermissionKind::Task),
        ToolCapability::ReadFs => None,
    }
}

fn mode_for_kind(
    permissions: &CategoryPermissions,
    kind: PermissionKind,
) -> Option<&PermissionMode> {
    match kind {
        PermissionKind::EditFs => permissions.edit.as_ref(),
        PermissionKind::Shell => permissions.shell.as_ref(),
        PermissionKind::Network => permissions.network.as_ref(),
        PermissionKind::Question => permissions.question.as_ref(),
        PermissionKind::Task => permissions.task.as_ref(),
        PermissionKind::WebFetch => permissions
            .webfetch
            .as_ref()
            .or(permissions.network.as_ref()),
        PermissionKind::WebSearch => permissions
            .websearch
            .as_ref()
            .or(permissions.network.as_ref()),
        PermissionKind::CodeSearch => permissions
            .codesearch
            .as_ref()
            .or(permissions.network.as_ref()),
        PermissionKind::Lsp => permissions.lsp.as_ref(),
    }
}

#[cfg(test)]
mod tests;
