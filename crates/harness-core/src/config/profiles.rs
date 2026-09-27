use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ProfileConfig {
    pub name: Option<String>,
    pub description: String,
    #[serde(alias = "systemPrompt", alias = "prompt")]
    pub system_prompt: Option<String>,
    #[serde(rename = "model_ref", alias = "modelRef", alias = "model")]
    pub model_ref: String,
    pub model_ref_explicit: bool,
    pub variant: Option<String>,
    pub temperature: Option<f32>,
    #[serde(alias = "topP")]
    pub top_p: Option<f32>,
    pub mode: AgentMode,
    pub hidden: bool,
    pub color: Option<String>,
    pub options: BTreeMap<String, serde_json::Value>,
    pub permissions: Option<ProfilePermissions>,
    #[serde(alias = "maxIters", alias = "steps", alias = "maxSteps")]
    pub max_iters: Option<usize>,
    #[serde(alias = "toolFailureMode")]
    pub tool_failure_mode: ToolFailureMode,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Primary,
    Subagent,
    #[default]
    All,
}
impl AgentMode {
    pub const fn is_subagent_only(self) -> bool {
        matches!(self, Self::Subagent)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Copy, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureMode {
    #[default]
    FailTurn,
    ContinueAsToolMessage,
}
