use harness_core::event::ActorKind;
use harness_core::perm::{permission_kind_for_tool_call, PermissionKind};
use harness_core::tool::{ToolCapability, ToolRegistry};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NativeToolCatalogEntry {
    pub canonical_id: String,
    pub provider_function_name: String,
    pub aliases: Vec<String>,
    pub description_summary: String,
    pub capability: ToolCapability,
    pub permission_kind: Option<String>,
    pub actor_availability: Vec<ActorKind>,
    pub supervisor_only: bool,
    pub schema_status: String,
    pub mutation: String,
    pub replay_behavior: String,
    pub artifact_behavior: String,
    pub docs_status: String,
    pub built_in: bool,
}

pub fn native_tool_catalog_entries(registry: &ToolRegistry) -> Vec<NativeToolCatalogEntry> {
    let mapping = registry.function_name_mapping();
    registry
        .tool_ids()
        .into_iter()
        .filter_map(|tool_id| {
            let tool = registry.get(&tool_id)?;
            let capability = tool.capability();
            let actor_availability = [
                ActorKind::Supervisor,
                ActorKind::Worker,
                ActorKind::System,
                ActorKind::User,
            ]
            .into_iter()
            .filter(|actor| registry.get_for_actor(*actor, &tool_id).is_some())
            .collect::<Vec<_>>();
            let supervisor_only = actor_availability == [ActorKind::Supervisor];
            let description_summary = summarize_description(tool.description());
            let permission_kind =
                permission_kind_for_tool_call(&tool_id, capability).map(canonical_permission_name);

            Some(NativeToolCatalogEntry {
                provider_function_name: mapping
                    .function_name_for_tool_id(&tool_id)
                    .unwrap_or(tool_id.as_str())
                    .to_string(),
                aliases: aliases_for_tool(&tool_id),
                description_summary,
                capability,
                permission_kind,
                actor_availability,
                supervisor_only,
                schema_status: schema_status(tool.parameters_json_schema()),
                mutation: mutation_status(&tool_id, capability).to_string(),
                replay_behavior: replay_behavior(&tool_id, capability).to_string(),
                artifact_behavior: artifact_behavior(&tool_id).to_string(),
                docs_status: "documented".to_string(),
                built_in: !tool_id.starts_with("mcp."),
                canonical_id: tool_id,
            })
        })
        .collect()
}

pub fn canonical_permission_name(kind: PermissionKind) -> String {
    match kind {
        PermissionKind::EditFs => "edit",
        PermissionKind::Shell => "bash",
        PermissionKind::Network => "network",
        PermissionKind::Question => "question",
        PermissionKind::Task => "task",
        PermissionKind::WebFetch => "webfetch",
        PermissionKind::WebSearch => "websearch",
        PermissionKind::CodeSearch => "codesearch",
        PermissionKind::Lsp => "lsp",
    }
    .to_string()
}

fn summarize_description(description: &str) -> String {
    let first_sentence = description
        .split('.')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(description.trim());
    if first_sentence.chars().count() <= 160 {
        first_sentence.to_string()
    } else {
        let mut text = first_sentence.chars().take(157).collect::<String>();
        text.push_str("...");
        text
    }
}

fn aliases_for_tool(tool_id: &str) -> Vec<String> {
    match tool_id {
        "task" => vec!["agent".to_string(), "subagent_type".to_string()],
        "background_output" => vec!["task_id".to_string(), "session_id".to_string()],
        "background_cancel" => vec!["background_output(cancel=true)".to_string()],
        "read" => vec!["filePath".to_string(), "path".to_string()],
        "lsp.rename" => vec!["rename_symbol".to_string()],
        _ => Vec::new(),
    }
}

fn schema_status(schema: serde_json::Value) -> String {
    if schema
        .get("additionalProperties")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        "strict".to_string()
    } else {
        "open".to_string()
    }
}

fn mutation_status(tool_id: &str, capability: ToolCapability) -> &'static str {
    if tool_id == "background_output" {
        return "read_cancel_compatibility";
    }
    if tool_id == "todowrite" {
        return "mutating";
    }
    if matches!(
        tool_id,
        "session_list" | "session_read" | "session_search" | "session_info" | "todoread"
    ) {
        return "read_only";
    }
    match capability {
        ToolCapability::EditFs | ToolCapability::Shell | ToolCapability::SpawnAgent => "mutating",
        ToolCapability::ReadFs | ToolCapability::Network => "read_only",
    }
}

fn replay_behavior(tool_id: &str, capability: ToolCapability) -> &'static str {
    if matches!(
        tool_id,
        "session_list" | "session_read" | "session_search" | "session_info"
    ) {
        return "projection_read_only";
    }
    match capability {
        ToolCapability::ReadFs => "local_read_only",
        ToolCapability::EditFs => "writes_events_and_artifacts",
        ToolCapability::Shell => "executes_host_command",
        ToolCapability::Network => "external_io_when_called",
        ToolCapability::SpawnAgent => "coordinator_control_plane",
    }
}

fn artifact_behavior(tool_id: &str) -> &'static str {
    match tool_id {
        "read" | "glob" | "grep" | "session_read" | "session_search" | "session_info"
        | "ast_grep_search" | "ast_grep_replace" => "spills_large_output",
        "edit" | "shell.run" | "bash" => "records_artifacts_when_large_or_applicable",
        _ => "summary_only",
    }
}

#[cfg(test)]
mod tests {
    use harness_core::config::ShellAllowlist;

    use crate::{coordinator_registry, native_tool_catalog_entries};

    #[test]
    fn catalog_includes_registered_tool_ids_with_permission_metadata() {
        let registry = coordinator_registry(ShellAllowlist::default());
        let catalog = native_tool_catalog_entries(&registry);
        let ids = catalog
            .iter()
            .map(|entry| entry.canonical_id.as_str())
            .collect::<Vec<_>>();

        for expected in [
            "task",
            "background_output",
            "background_cancel",
            "session_list",
            "session_read",
            "session_search",
            "session_info",
            "ast_grep_search",
            "ast_grep_replace",
        ] {
            assert!(ids.contains(&expected), "missing catalog entry {expected}");
        }

        let background_cancel = catalog
            .iter()
            .find(|entry| entry.canonical_id == "background_cancel")
            .expect("background_cancel");
        assert_eq!(background_cancel.permission_kind.as_deref(), Some("task"));
        assert_eq!(background_cancel.schema_status, "strict");

        let ast_grep = catalog
            .iter()
            .find(|entry| entry.canonical_id == "ast_grep_search")
            .expect("ast_grep_search");
        assert_eq!(ast_grep.permission_kind.as_deref(), Some("codesearch"));
        assert_eq!(ast_grep.mutation, "read_only");

        let ast_grep_replace = catalog
            .iter()
            .find(|entry| entry.canonical_id == "ast_grep_replace")
            .expect("ast_grep_replace");
        assert_eq!(ast_grep_replace.permission_kind.as_deref(), Some("edit"));
        assert_eq!(ast_grep_replace.mutation, "mutating");
        assert_eq!(ast_grep_replace.artifact_behavior, "spills_large_output");

        let session_info = catalog
            .iter()
            .find(|entry| entry.canonical_id == "session_info")
            .expect("session_info");
        assert_eq!(session_info.replay_behavior, "projection_read_only");
        assert_eq!(session_info.artifact_behavior, "spills_large_output");

        let todo_write = catalog
            .iter()
            .find(|entry| entry.canonical_id == "todowrite")
            .expect("todowrite");
        assert_eq!(todo_write.permission_kind.as_deref(), Some("task"));
        assert_eq!(todo_write.mutation, "mutating");
    }
}
