use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionCapabilityDescriptor {
    pub id: String,
    #[serde(default = "default_true")]
    pub default_enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}
fn default_true() -> bool {
    true
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionToolDescriptor {
    pub id: String,
    pub capability_id: String,
    pub permission: ExtensionToolPermission,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionToolPermission {
    Bash,
    Edit,
    Question,
    Task,
    Webfetch,
    Websearch,
    Codesearch,
    Lsp,
}
impl ExtensionToolPermission {
    pub fn public_name(&self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Edit => "edit",
            Self::Question => "question",
            Self::Task => "task",
            Self::Webfetch => "webfetch",
            Self::Websearch => "websearch",
            Self::Codesearch => "codesearch",
            Self::Lsp => "lsp",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionHookDescriptor {
    pub id: String,
    pub capability_id: String,
    pub lifecycle_event: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub replay_label: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionCommandDescriptor {
    pub id: String,
    pub capability_id: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub replay_label: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionPromptDescriptor {
    pub id: String,
    pub capability_id: String,
    #[serde(default)]
    pub replay_label: Option<String>,
}
pub type ExtensionDiagnosticDescriptor = ExtensionPromptDescriptor;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionMcpBundleDescriptor {
    pub id: String,
    pub capability_id: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub server_ids: Vec<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionProviderDecoratorDescriptor {
    pub id: String,
    pub capability_id: String,
    pub status: ExtensionSeamStatus,
    #[serde(default)]
    pub provider_scope: Option<String>,
    #[serde(default)]
    pub replay_label: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionSeamStatus {
    Native,
    Fallback,
    IntentionallyUnsupported,
    PostV1,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionReplayDescriptor {
    pub label: String,
    pub summary_template: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReplayMetadata {
    pub schema_version: String,
    pub extension_id: String,
    pub display_name: Option<String>,
    pub capability_ids: Vec<String>,
    pub disabled_capability_ids: Vec<String>,
    pub tool_descriptor_count: usize,
    pub hook_descriptor_count: usize,
    pub command_descriptor_count: usize,
    pub prompt_descriptor_count: usize,
    pub mcp_bundle_descriptor_count: usize,
    pub diagnostic_descriptor_count: usize,
    pub provider_decorator_descriptor_count: usize,
    pub replay_label: Option<String>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifestRuntimeEffects {
    pub registers_tools: bool,
    pub executes_commands: bool,
    pub launches_mcp: bool,
    pub invokes_provider_decorators: bool,
    pub loads_external_code: bool,
    pub mutates_sessions: bool,
}
impl ExtensionManifestRuntimeEffects {
    pub fn descriptor_only() -> Self {
        Self::default()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionManifestSummary {
    pub extension_id: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub capabilities: usize,
    pub enabled_capabilities: usize,
    pub tools: usize,
    pub hooks: usize,
    pub commands: usize,
    pub prompts: usize,
    pub mcp_bundles: usize,
    pub diagnostics: usize,
    pub provider_decorators: usize,
    pub loads_external_code: bool,
}
impl ExtensionManifestSummary {
    pub fn one_line(&self) -> String {
        format!("extension descriptor: id=`{}` caps={}/{} tools={} hooks={} commands={} prompts={} mcp={} diag={} decorators={} loads_code={}", self.extension_id, self.enabled_capabilities, self.capabilities, self.tools, self.hooks, self.commands, self.prompts, self.mcp_bundles, self.diagnostics, self.provider_decorators, self.loads_external_code)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionDiscoverSummary {
    pub discovered: usize,
    pub loads_external_code: bool,
}
impl ExtensionDiscoverSummary {
    pub fn one_line(&self) -> String {
        format!(
            "extension discover: {} descriptor(s) (loads_code={})",
            self.discovered, self.loads_external_code
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ExtensionLoadOutcome {
    Loaded { path: String, extension_id: String },
    Failed { path: String, reason: String },
}
impl ExtensionLoadOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Loaded { path, extension_id } => {
                format!("extension load: ok id=`{extension_id}` path=`{path}` (loads_code=false)")
            }
            Self::Failed { path, reason } => {
                format!("extension load: failed path=`{path}` ({reason})")
            }
        }
    }
}
