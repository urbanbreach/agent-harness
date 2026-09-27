use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModelRef {
    pub provider_id: String,
    pub model_id: String,
}
impl AgentModelRef {
    pub fn parse(value: &str) -> Self {
        let (provider, model) = value
            .split_once(':')
            .or_else(|| value.split_once('/'))
            .unwrap_or(("mock", value));
        Self {
            provider_id: provider.into(),
            model_id: model.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderCompactionSummarySource {
    pub strategy: String,
    pub model_ref: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub previous_summary_used: bool,
    pub model_backed: bool,
    pub deterministic_fallback: bool,
    pub summary_contract_version: Option<u32>,
    pub summary_contract_enforced: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct AgentProfile {
    pub name: String,
    pub model_ref: String,
    pub model_ref_explicit: bool,
    pub system_prompt: String,
    pub temperature: Option<f32>,
    pub cache_retention: harness_providers::CacheRetention,
    pub max_iters: Option<usize>,
    pub tool_failure_mode: crate::config::ToolFailureMode,
    pub toolset: Vec<String>,
    pub permission_ruleset: crate::perm::PermissionRuleset,
}
impl AgentProfile {
    pub fn fallback(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            model_ref: "mock:default".into(),
            model_ref_explicit: false,
            system_prompt: String::new(),
            temperature: None,
            cache_retention: Default::default(),
            max_iters: None,
            tool_failure_mode: crate::config::ToolFailureMode::ContinueAsToolMessage,
            toolset: Vec::new(),
            permission_ruleset: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentModelSettings {
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
    pub thinking: Option<serde_json::Value>,
}
impl From<&crate::config::ResolvedModelTarget> for AgentModelSettings {
    fn from(target: &crate::config::ResolvedModelTarget) -> Self {
        Self {
            variant: target.variant.clone(),
            reasoning_effort: target.reasoning_effort.clone(),
            text_verbosity: target.text_verbosity.clone(),
            reasoning_summary: target.reasoning_summary.clone(),
            thinking: target.thinking.clone(),
        }
    }
}
