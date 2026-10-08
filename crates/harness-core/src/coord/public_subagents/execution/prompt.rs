use super::*;
use crate::system_prompt::{PromptContext, PromptSource};

impl Runtime {
    pub(in crate::coord) fn native_prompt_source(&self, agent: &str) -> Option<PromptSource> {
        let child = self.native_subagents.get(agent)?;
        let resolved = child.resolved.as_ref()?;
        let root = self.agents.get(&child.registration.root_agent)?;
        let mut suffix = self
            .config
            .agent_prompt_sources
            .get(&root.profile.name)
            .map(|source| source.suffix.clone())
            .unwrap_or_default();
        if let Some(schema) = &child.registration.output_schema {
            suffix.push_str("\n\n");
            suffix.push_str(&crate::subagent::output_contract::instructions(schema));
        }
        Some(PromptSource {
            user_prompt_dir: self
                .config
                .agent_prompt_sources
                .get(&root.profile.name)
                .and_then(|source| source.user_prompt_dir.clone()),
            project_prompt_root: Some(root.cwd.clone()),
            subagent: true,
            agent_instructions: resolved.definition.prompt_body.clone().unwrap_or_default(),
            role_instructions: resolved.role_prompt.clone().unwrap_or_default(),
            persona_instructions: resolved.persona_instructions.clone().unwrap_or_default(),
            isolated: resolved.isolation == SubagentIsolationMode::Worktree,
            tool_kinds: resolved
                .tools
                .iter()
                .filter_map(|tool| {
                    tool.kind
                        .map(|kind| (tool.id.clone(), kind_name(kind).into()))
                })
                .collect(),
            suffix,
            ..Default::default()
        })
    }

    pub(super) fn native_system_prompt(
        &self,
        agent: &str,
        cwd: &Path,
    ) -> Result<Option<String>, CoordinatorError> {
        let Some(source) = self.native_prompt_source(agent) else {
            return Ok(None);
        };
        let state = &self.agents[agent];
        let permissions = (&self.config.permission_policy, &state.policy);
        let scope = self.tool_scope(Some(agent));
        let mut available = self.config.tool_registry.prompt_definitions(
            &state.profile,
            scope.as_deref(),
            permissions,
        );
        let mut direct =
            self.config
                .tool_registry
                .definitions(&state.profile, scope.as_deref(), permissions);
        let hints = self.native_subagent_schema(agent)?;
        for tools in [&mut available, &mut direct] {
            crate::coord::public_subagent_hooks::apply_native_schema_hints(tools, &hints, true);
        }
        source
            .render(&PromptContext {
                workspace: cwd,
                model: &state.info.model_ref,
                prompt_preset: state
                    .target
                    .as_ref()
                    .map(|target| target.resolution.prompt_preset.as_str())
                    .unwrap_or_default(),
                tools: &available,
                direct_tools: &direct,
                current_date: &self.clock.system_time_rfc3339().unwrap_or_default(),
                max_concurrent: self.config.subagents.max_concurrent,
                limit_behavior: self.config.subagents.limit_behavior,
                behavior: &self.config.behavior,
            })
            .map(|rendered| Some(rendered.system))
            .map_err(|error| native_invalid(format!("subagent prompt: {error}")))
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
