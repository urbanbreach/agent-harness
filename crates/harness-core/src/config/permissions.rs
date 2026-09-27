use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProfilePermissions {
    #[serde(rename = "*")]
    pub fallback: Option<PermissionMode>,
    pub edit: Option<PermissionMode>,
    #[serde(alias = "bash")]
    pub shell: Option<PermissionMode>,
    pub network: Option<PermissionMode>,
    pub question: Option<PermissionMode>,
    pub task: Option<PermissionMode>,
    pub todowrite: Option<PermissionMode>,
    #[serde(alias = "webFetch")]
    pub webfetch: Option<PermissionMode>,
    #[serde(alias = "webSearch")]
    pub websearch: Option<PermissionMode>,
    #[serde(alias = "codeSearch")]
    pub codesearch: Option<PermissionMode>,
    #[serde(alias = "codeLsp")]
    pub lsp: Option<PermissionMode>,
    pub read: Option<PermissionMode>,
    pub external_directory: Option<PermissionMode>,
    pub doom_loop: Option<PermissionMode>,
    pub rules: PermissionRuleSet,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionsConfig {
    pub defaults: PermissionDefaultsConfig,
    #[serde(rename = "*")]
    pub fallback: Option<PermissionMode>,
    pub rules: PermissionRuleSet,
    #[serde(rename = "shell_allowlist", alias = "shellAllowlist")]
    pub shell_allowlist: ShellAllowlist,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionDefaultsConfig {
    pub edit: PermissionMode,
    pub shell: PermissionMode,
    pub network: PermissionMode,
    pub question: Option<PermissionMode>,
    pub task: Option<PermissionMode>,
    #[serde(alias = "webFetch")]
    pub webfetch: Option<PermissionMode>,
    #[serde(alias = "webSearch")]
    pub websearch: Option<PermissionMode>,
    #[serde(alias = "codeSearch")]
    pub codesearch: Option<PermissionMode>,
    #[serde(alias = "codeLsp")]
    pub lsp: Option<PermissionMode>,
    pub read: Option<PermissionMode>,
    pub external_directory: Option<PermissionMode>,
    pub doom_loop: Option<PermissionMode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PermissionRuleSet {
    pub shell: Vec<PermissionSelectorRule>,
    pub edit: Vec<PermissionSelectorRule>,
    pub task: Vec<PermissionSelectorRule>,
    pub read: Vec<PermissionSelectorRule>,
    pub external_directory: Vec<PermissionSelectorRule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
pub struct PermissionSelectorRule {
    pub selector: PermissionSelector,
    pub mode: PermissionMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionSelector {
    Exact(String),
    Prefix(String),
    Glob(String),
    CatchAll,
}
impl From<String> for PermissionSelector {
    fn from(value: String) -> Self {
        Self::Glob(value)
    }
}
impl From<&str> for PermissionSelector {
    fn from(value: &str) -> Self {
        Self::Glob(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShellAllowlistMode {
    #[default]
    PermissionPatterns,
    LegacyExecutables,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ShellAllowlist {
    #[serde(alias = "policy_mode", alias = "policyMode")]
    pub mode: ShellAllowlistMode,
    pub executables: Vec<String>,
    #[serde(rename = "cwd_roots", alias = "cwdRoots")]
    pub cwd_roots: Vec<String>,
}
