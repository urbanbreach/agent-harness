use super::*;
use minijinja::{syntax::SyntaxConfig, Environment};
use serde_json::json;

impl Runtime {
    pub(super) fn native_system_prompt(
        &self,
        agent: &str,
        cwd: &Path,
    ) -> Result<Option<String>, CoordinatorError> {
        let child = &self.native_subagents[agent];
        if child.registration.source.as_deref() == Some(agent) {
            return Ok(None);
        }
        let Some(resolved) = &child.resolved else {
            return Err(native_invalid(
                "subagent prompt definition is unavailable".into(),
            ));
        };
        let state = &self.agents[agent];
        let mut available = self.config.tool_registry.definitions(
            &state.profile,
            self.tool_scope(Some(agent)).as_deref(),
            (&self.config.permission_policy, &state.policy),
        );
        crate::coord::public_subagent_hooks::apply_native_schema_hints(
            &mut available,
            &json!({"depth":self.native_depth(agent), "max_depth":self.config.subagents.max_depth}),
            true,
        );
        let mut by_kind = BTreeMap::new();
        let mut params = BTreeMap::new();
        for tool in &resolved.tools {
            let Some(kind) = tool.kind else { continue };
            let Some(definition) = available.iter().find(|item| item.tool_id == tool.id) else {
                continue;
            };
            let kind = kind_name(kind);
            if by_kind.contains_key(kind) {
                continue;
            }
            by_kind.insert(kind, tool.id.as_str());
            let mut names = BTreeMap::new();
            if let Some(properties) = definition.parameters["properties"].as_object() {
                names.extend(properties.keys().map(|name| (name.clone(), name.clone())));
                if kind == "execute" && properties.contains_key("run_in_background") {
                    names.insert("is_background".into(), "run_in_background".into());
                }
            }
            params.insert(kind, names);
        }
        let date = self.clock.system_time_rfc3339().unwrap_or_default();
        let context = json!({
            "tools": {"by_kind": by_kind}, "params": params,
            "os_name": std::env::consts::OS,
            "shell_path": std::env::var("SHELL").unwrap_or_default(),
            "working_directory": cwd.to_string_lossy(),
            "current_date": date.split('T').next().unwrap_or_default(),
            "role_instructions": resolved.role_prompt.as_deref().unwrap_or_default(),
            "persona_instructions": resolved.persona_instructions.as_deref().unwrap_or_default(),
            "memory_enabled": by_kind.contains_key("memory_search") && by_kind.contains_key("memory_get"),
            "is_windows": cfg!(windows), "has_unix_utilities": cfg!(unix),
            "system_reminders_enabled": true,
        });
        let syntax = SyntaxConfig::builder()
            .block_delimiters("${%", "%}")
            .variable_delimiters("${{", "}}")
            .comment_delimiters("${#", "#}")
            .build()
            .map_err(|error| native_invalid(error.to_string()))?;
        let mut environment = Environment::new();
        environment.set_syntax(syntax);
        let mut prompt = environment
            .render_str(
                include_str!("../../../../../../.agent-harness/subagent-prompts/base.md"),
                &context,
            )
            .map_err(|error| native_invalid(error.to_string()))?;
        if let Some(body) = &resolved.definition.prompt_body {
            prompt.push_str("\n\n");
            prompt.push_str(
                &environment
                    .render_str(body, &context)
                    .unwrap_or_else(|_| body.clone()),
            );
        }
        Ok(Some(prompt))
    }
}

fn kind_name(kind: SubagentToolKind) -> &'static str {
    use SubagentToolKind as Kind;
    match kind {
        Kind::Read => "read",
        Kind::ListDir | Kind::List => "list",
        Kind::Search => "search",
        Kind::Lsp => "lsp",
        Kind::Plan => "plan",
        Kind::MemorySearch => "memory_search",
        Kind::MemoryGet => "memory_get",
        Kind::WebSearch => "web_search",
        Kind::WebFetch => "web_fetch",
        Kind::BackgroundTaskAction => "background_task_action",
        Kind::KillTaskAction => "kill_task_action",
        Kind::Task => "task",
        Kind::EnterPlan => "enter_plan",
        Kind::ExitPlan => "exit_plan",
        Kind::AskUser => "ask_user",
        Kind::Skill => "skill",
        Kind::Edit => "edit",
        Kind::Write => "write",
        Kind::Delete => "delete",
        Kind::Move => "move",
        Kind::Feedback => "feedback",
        Kind::ImageGen => "image_gen",
        Kind::VideoGen => "video_gen",
        Kind::ImageToVideo => "image_to_video",
        Kind::ReferenceToVideo => "reference_to_video",
        Kind::Execute => "execute",
        Kind::ActiveAgentMessage => "active_agent_message",
        Kind::Workflow => "workflow",
    }
}
