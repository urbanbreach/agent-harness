//! Native tools submit to coordinator policy and return bounded results.
use harness_core::{config::ShellAllowlist, tool::ToolRegistry};
use std::sync::Arc;
mod ast_grep;
mod batch;
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
mod tasks;
mod todos;
mod web;
pub use harness_core::UnwrapOrAbort;
pub use skills::{
    discover_skill_catalog, discover_skill_catalog_with_config, SkillCatalog, SkillCatalogEntry,
    SkillCatalogStatus,
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
    registry.register(Arc::new(tasks::TaskTool(skills)));
    registry.register(Arc::new(tasks::BackgroundTool::Output));
    registry.register(Arc::new(tasks::BackgroundTool::Cancel));
    registry.register(Arc::new(batch::BatchTool));
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
