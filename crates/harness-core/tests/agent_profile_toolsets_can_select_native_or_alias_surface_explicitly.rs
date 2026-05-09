use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::tool::{
    canonical_tool_id_for, Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult,
};
use serde_json::Value;

struct StaticTool(&'static str);

#[async_trait]
impl Tool for StaticTool {
    fn id(&self) -> &str {
        self.0
    }

    fn capability(&self) -> ToolCapability {
        ToolCapability::ReadFs
    }

    async fn call(&self, _ctx: ToolContext, _args_json: Value) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text("ok"))
    }
}

#[test]
fn agent_profile_toolsets_are_exported_as_single_surface_provider_defs() {
    let registry = test_tool_registry();

    let defs = build_provider_tool_defs(&test_profile(), &registry)
        .expect("single-surface tool defs should build");

    let tool_ids = tool_ids(&defs);

    assert_eq!(tool_ids, vec!["bash", "list", "read"]);
    assert_eq!(function_names(&defs), vec!["bash", "list", "read"]);
    assert_eq!(
        tool_ids
            .iter()
            .map(|tool_id| {
                canonical_tool_id_for(tool_id)
                    .unwrap_or(tool_id)
                    .to_string()
            })
            .collect::<BTreeSet<_>>(),
        tool_ids.iter().map(|tool_id| tool_id.to_string()).collect()
    );
    assert_eq!(
        tool_ids.iter().collect::<BTreeSet<_>>().len(),
        tool_ids.len()
    );
}

fn test_profile() -> AgentProfile {
    AgentProfile {
        name: "worker".to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        system_prompt: "sys".to_string(),
        max_iters: Some(12),
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        toolset: vec!["read".to_string(), "bash".to_string(), "list".to_string()],
    }
}

fn test_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for tool_id in ["read", "bash", "list"] {
        registry.register(Arc::new(StaticTool(tool_id)));
    }
    registry
}

fn tool_ids(tool_defs: &[harness_providers::ToolDef]) -> Vec<&str> {
    tool_defs.iter().map(|tool| tool.tool_id.as_str()).collect()
}

fn function_names(tool_defs: &[harness_providers::ToolDef]) -> Vec<&str> {
    tool_defs
        .iter()
        .map(|tool| tool.function_name.as_str())
        .collect()
}
