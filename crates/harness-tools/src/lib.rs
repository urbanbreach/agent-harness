//! Native tools submit to coordinator policy and return bounded results.
use harness_core::{config::ShellAllowlist, tool::ToolRegistry};
use std::sync::Arc;
mod ast_grep;
mod eval;
pub use eval::register as register_eval_tool;
mod files;
mod formatters;
mod github;
pub use github::register_github_tools;
mod hashline;
mod lsp;
pub use lsp::register_lsp_tools;
mod mcp;
mod media;
pub use mcp::{register_mcp_tools, register_remote_search_tools};
mod patch;
mod process;
mod search;
mod sessions;
mod shell;
pub use shell::register_shell_tool;
mod skills;
mod subagents;
pub use subagents::{SubagentOperation, SubagentTool};
mod todos;
mod web;
pub use harness_core::UnwrapOrAbort;
pub use skills::{
    discover_skill_catalog, discover_skill_catalog_with_config, NativeSkillCatalogDiscovery,
    SkillCatalog, SkillCatalogEntry, SkillCatalogStatus,
};

pub fn coordinator_registry(shell_allowlist: ShellAllowlist) -> ToolRegistry {
    let mut registry = coordinator_registry_with_skills(
        shell_allowlist,
        harness_core::config::registered_skills_config(),
    );
    if let Some(config) = harness_core::config::registered_integrations_config() {
        register_remote_search_tools(&mut registry, config.remote_search);
    }
    registry
}
pub fn coordinator_registry_with_skills(
    shell_allowlist: ShellAllowlist,
    skills: harness_core::config::SkillsConfig,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    for tool in [
        files::FileTool::Read,
        files::FileTool::Write,
        files::FileTool::Edit,
        files::FileTool::List,
    ] {
        registry.register(Arc::new(tool));
    }
    register_shell_tool(&mut registry, shell_allowlist, &|key| {
        std::env::var(key).ok()
    });
    registry.register(Arc::new(search::SearchTool::Glob));
    registry.register(Arc::new(search::SearchTool::Grep));
    registry.register(Arc::new(ast_grep::AstTool::Search));
    registry.register(Arc::new(ast_grep::AstTool::Replace));
    registry.register(Arc::new(patch::PatchTool));
    registry.register(Arc::new(harness_core::tool::QuestionTool));
    registry.register(Arc::new(skills::load::SkillTool(skills.clone())));
    register_subagent_tools(
        &mut registry,
        &harness_core::config::SubagentRuntimeConfig::default(),
        &harness_core::config::SubagentDefinitionSnapshot::default(),
        None,
    );
    register_eval_tool(&mut registry, Default::default());
    registry.register(Arc::new(todos::TodoTool::Read));
    registry.register(Arc::new(todos::TodoTool::Write));
    registry.register(Arc::new(web::Fetch));
    register_github_tools(&mut registry, &|key| std::env::var(key).ok());
    register_remote_search_tools(&mut registry, Default::default());
    register_lsp_tools(&mut registry, harness_core::config::registered_lsp_config());
    for tool in [
        sessions::SessionTool::List,
        sessions::SessionTool::Read,
        sessions::SessionTool::Search,
        sessions::SessionTool::Info,
    ] {
        registry.register(Arc::new(tool));
    }
    registry
}

/// Replace native public operations from a resolved startup snapshot.
/// Read/wait/kill remain available for commands when spawning is disabled.
pub fn register_subagent_tools(
    registry: &mut ToolRegistry,
    settings: &harness_core::config::SubagentRuntimeConfig,
    definitions: &harness_core::config::SubagentDefinitionSnapshot,
    catalog: Option<&harness_core::config::SubagentModelCatalog>,
) {
    use harness_core::subagent::{
        GetCommandOrSubagentOutputInput, KillCommandOrSubagentInput, SendSubagentMessageInput,
        WaitCommandsOrSubagentsInput,
    };
    for id in [
        "spawn_subagent",
        "get_command_or_subagent_output",
        "wait_commands_or_subagents",
        "kill_command_or_subagent",
        "send_subagent_message",
        "task",
        "get_task_output",
        "wait_tasks",
        "kill_task",
    ] {
        registry.remove(id);
    }
    let mut operations: Vec<(SubagentOperation, serde_json::Value, &str)> = vec![
        (
            SubagentOperation::Output,
            schemars::schema_for!(GetCommandOrSubagentOutputInput).into(),
            "Read command or subagent output; optionally wait for completion.",
        ),
        (
            SubagentOperation::Wait,
            schemars::schema_for!(WaitCommandsOrSubagentsInput).into(),
            "Wait for commands or subagents without cancelling their work.",
        ),
        (
            SubagentOperation::Kill,
            schemars::schema_for!(KillCommandOrSubagentInput).into(),
            "Cancel the specified command or subagent.",
        ),
    ];
    if settings.enabled {
        operations.push((
            SubagentOperation::Spawn,
            harness_core::subagent::spawn_subagent_schema(settings, definitions, catalog, None),
            "Spawn a subagent in the background by default; request foreground for its result.",
        ));
    }
    if settings.messaging_enabled {
        operations.push((
            SubagentOperation::Send,
            schemars::schema_for!(SendSubagentMessageInput).into(),
            "Send a message using the authenticated caller's subagent routing grants.",
        ));
    }
    for (operation, schema, description) in operations {
        let tool = SubagentTool::new(operation, schema, description.into());
        if let Some(alias) = tool.reference_alias() {
            registry.register(Arc::new(alias));
        }
        registry.register(Arc::new(tool));
    }
}
