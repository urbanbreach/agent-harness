use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::agent::{build_provider_tool_defs, AgentProfile};
use harness_core::tool::{
    canonical_tool_id_for, Tool, ToolCapability, ToolContext, ToolError, ToolRegistry, ToolResult,
    ToolSurface,
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
fn agent_profile_toolsets_can_select_native_or_alias_surface_explicitly() {
    let registry = test_tool_registry();

    let native_defs = build_provider_tool_defs(&test_profile(ToolSurface::Native), &registry)
        .expect("native tool defs should build");
    let compat_defs = build_provider_tool_defs(&test_profile(ToolSurface::Compat), &registry)
        .expect("compat tool defs should build");

    let native_ids = tool_ids(&native_defs);
    let compat_ids = tool_ids(&compat_defs);

    assert_eq!(native_ids, vec!["fs.ls", "fs.read", "shell.run"]);
    assert_eq!(compat_ids, vec!["bash", "fs.ls", "read"]);

    assert_eq!(
        function_names(&native_defs),
        vec!["fs_ls", "fs_read", "shell_run"]
    );
    assert_eq!(function_names(&compat_defs), vec!["bash", "fs_ls", "read"]);

    assert_eq!(
        canonical_behaviors(&native_ids),
        canonical_behaviors(&compat_ids)
    );
    assert_eq!(
        native_ids.iter().collect::<BTreeSet<_>>().len(),
        native_ids.len()
    );
    assert_eq!(
        compat_ids.iter().collect::<BTreeSet<_>>().len(),
        compat_ids.len()
    );
}

fn test_profile(tool_surface: ToolSurface) -> AgentProfile {
    AgentProfile {
        name: "worker".to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        system_prompt: "sys".to_string(),
        max_iters: 12,
        temperature: Some(0.0),
        tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
        tool_surface,
        toolset: vec![
            "read".to_string(),
            "fs.read".to_string(),
            "bash".to_string(),
            "shell.run".to_string(),
            "fs.ls".to_string(),
        ],
    }
}

fn test_tool_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for tool_id in ["fs.read", "read", "shell.run", "bash", "fs.ls"] {
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

fn canonical_behaviors(tool_ids: &[&str]) -> BTreeSet<String> {
    tool_ids
        .iter()
        .map(|tool_id| {
            canonical_tool_id_for(tool_id)
                .unwrap_or(tool_id)
                .to_string()
        })
        .collect()
}
