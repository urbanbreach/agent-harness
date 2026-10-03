mod files;
mod question;
use crate::{
    agent::AgentModelSettings, coord::CoordinatorHandle, event::EventActor, ids::ToolCallId,
};
pub use files::{resolve_file_path, ToolRunState};
pub(crate) use question::QuestionRequest;
pub use question::QuestionTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCapability {
    ReadFs,
    EditFs,
    Shell,
    Network,
    SpawnAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub path: String,
    pub digest: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub display_text: String,
    pub structured_json: Option<Value>,
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
}
pub(crate) fn provider_text(text: String, data: Option<Value>) -> String {
    match data {
        Some(data) => serde_json::json!({"text":text,"data":data}).to_string(),
        None => text,
    }
}
impl ToolResult {
    pub fn is_error(&self) -> bool {
        self.structured_json
            .as_ref()
            .and_then(|v| v.get("is_error"))
            .and_then(Value::as_bool)
            == Some(true)
    }
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            display_text: text.into(),
            ..Self::default()
        }
    }
    pub fn structured(text: impl Into<String>, value: Value) -> Self {
        Self {
            structured_json: Some(value),
            ..Self::text(text)
        }
    }
    pub fn artifacts(text: impl Into<String>, artifacts: Vec<ArtifactRef>) -> Self {
        Self {
            artifacts,
            ..Self::text(text)
        }
    }
    pub fn structured_with_artifacts(
        text: impl Into<String>,
        value: Value,
        artifacts: Vec<ArtifactRef>,
    ) -> Self {
        Self {
            artifacts,
            ..Self::structured(text, value)
        }
    }
    pub fn with_attachments(
        mut self,
        attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
    ) -> Self {
        self.attachments = attachments;
        self
    }
}
#[derive(Clone)]
pub struct ToolContext {
    pub run_id: String,
    pub workspace_root: PathBuf,
    pub(crate) policy_roots: Vec<PathBuf>,
    pub artifacts_dir: PathBuf,
    pub actor: EventActor,
    pub profile: Option<String>,
    pub tool_call_id: ToolCallId,
    pub current_model_ref: Option<String>,
    pub current_model_settings: Option<AgentModelSettings>,
    pub skill_startup: Option<Arc<crate::config::SkillStartupSnapshot>>,
    pub coordinator: CoordinatorHandle,
    pub cancellation: CancellationToken,
    pub tool_state: ToolRunState,
    pub formatter: Arc<crate::config::FormatterConfig>,
    pub redactor: Arc<dyn crate::redact::Redactor + Send + Sync>,
    pub external_directory_allow_prefixes: Vec<PathBuf>,
    pub(crate) approved_paths: Vec<(PathBuf, PathBuf)>,
}
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{code}: {message}")]
    Custom { code: String, message: String },
    #[error("tool argument error: {0}")]
    InvalidArguments(String),
    #[error("tool execution failed: {0}")]
    Execution(String),
    #[error("tool was cancelled")]
    Cancelled,
    #[error("remote server returned HTTP {status}")]
    HttpStatus {
        status: u16,
        retry_after_ms: Option<u64>,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    fn parameters_json_schema(&self) -> Value;
    fn capability(&self) -> ToolCapability;
    fn filesystem_paths(&self, _args: &Value) -> Result<Vec<PathBuf>, ToolError> {
        Ok(Vec::new())
    }
    fn permission_requests(&self, args: &Value) -> Vec<(String, String)> {
        let input = ["command", "path", "file_path", "url", "query"]
            .into_iter()
            .find_map(|key| args.get(key).and_then(Value::as_str));
        vec![(
            self.id().into(),
            input.map_or_else(|| args.to_string(), str::to_owned),
        )]
    }
    /// Parsed command prefixes eligible for a remembered shell approval.
    fn permission_always_patterns(&self, _args: &Value) -> Vec<String> {
        Vec::new()
    }
    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError>;
    /// Configured literal credentials only; this accessor must not perform I/O.
    fn secret_values(&self) -> Vec<String> {
        Vec::new()
    }
    async fn close_run(&self, _run_id: &str) -> Result<(), ToolError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
    descriptions: BTreeMap<(String, String), String>,
    catalogs: BTreeMap<String, ToolCatalog>,
}
/// Descriptors obtained by an approved discovery call; reading a catalog never performs I/O.
pub type ToolCatalog = Arc<RwLock<BTreeMap<String, Arc<dyn Tool>>>>;
impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.id().into(), tool);
    }
    pub fn remove(&mut self, id: &str) {
        let _ = self.tools.remove(id);
    }
    pub fn get(&self, id: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(id).cloned().or_else(|| {
            self.catalogs
                .values()
                .find_map(|catalog| catalog.read().ok()?.get(id).cloned())
        })
    }
    pub fn register_catalog(&mut self, permission_anchor: String, catalog: ToolCatalog) {
        self.catalogs.insert(permission_anchor, catalog);
    }
    pub(crate) fn allows(&self, profile: &crate::agent::AgentProfile, id: &str) -> bool {
        profile.toolset.iter().any(|allowed| {
            allowed == id
                || self
                    .catalogs
                    .get(allowed)
                    .is_some_and(|catalog| catalog.read().is_ok_and(|tools| tools.contains_key(id)))
        })
    }
    pub fn tool_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.tools.keys().cloned().collect();
        for catalog in self.catalogs.values() {
            if let Ok(tools) = catalog.read() {
                ids.extend(tools.keys().cloned());
            }
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }
    pub(crate) async fn close_run(&self, run: &str) -> Result<(), ToolError> {
        let mut result = Ok(());
        for tool in self.tools.values() {
            if let Err(error) = tool.close_run(run).await {
                result = Err(error);
            }
        }
        result
    }
    pub(crate) fn secret_values(&self) -> impl Iterator<Item = String> + '_ {
        self.tools.values().flat_map(|tool| tool.secret_values())
    }
    pub fn set_profile_tool_description(
        &mut self,
        tool: impl Into<String>,
        profile: impl Into<String>,
        description: impl Into<String>,
    ) {
        self.descriptions
            .insert((tool.into(), profile.into()), description.into());
    }
    pub fn description_for_profile(&self, tool: &str, profile: &str) -> Option<&str> {
        self.descriptions
            .get(&(tool.into(), profile.into()))
            .map(String::as_str)
    }
    pub(crate) fn definitions(
        &self,
        profile: &crate::agent::AgentProfile,
        permissions: (
            &crate::perm::PermissionPolicy,
            &crate::perm::PermissionPolicy,
        ),
    ) -> Vec<harness_providers::ToolDef> {
        let mut seen = std::collections::BTreeSet::new();
        let mut tools: Vec<_> = profile
            .toolset
            .iter()
            .filter_map(|id| self.get(id))
            .filter(|tool| seen.insert(tool.id().to_owned()))
            .collect();
        for id in &profile.toolset {
            let Some(catalog) = self.catalogs.get(id) else {
                continue;
            };
            let Ok(catalog) = catalog.read() else {
                continue;
            };
            let available = harness_providers::MAX_TOOL_DEFINITIONS.saturating_sub(tools.len());
            tools.extend(
                catalog
                    .values()
                    .filter(|tool| seen.insert(tool.id().to_owned()))
                    .take(available)
                    .cloned(),
            );
        }
        tools
            .into_iter()
            .filter(|tool| {
                tool.permission_requests(&serde_json::json!({}))
                    .iter()
                    .all(|(name, _)| {
                        !permissions.0.always_denies(name) && !permissions.1.always_denies(name)
                    })
            })
            .map(|tool| harness_providers::ToolDef {
                tool_id: tool.id().into(),
                function_name: harness_providers::tool_function_name(tool.id()),
                description: Some(
                    self.description_for_profile(tool.id(), &profile.name)
                        .unwrap_or_else(|| tool.description())
                        .into(),
                ),
                parameters: tool.parameters_json_schema(),
            })
            .collect()
    }
}
