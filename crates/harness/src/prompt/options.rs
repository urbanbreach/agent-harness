use harness_core::{
    config::{HarnessConfig, PermissionDefaultsConfig, PermissionMode},
    coord::CoordinatorConfig,
};

#[derive(clap::Args)]
pub(super) struct Options {
    #[arg(long, short = 'm')]
    model: Option<String>,
    #[arg(long)]
    variant: Option<String>,
    #[arg(long)]
    pub thinking: bool,
    #[arg(long, value_parser = ["none", "minimal", "low", "medium", "high", "xhigh", "max"])]
    reasoning_effort: Option<String>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    max_turns: Option<u32>,
    #[arg(long)]
    no_subagents: bool,
    #[arg(long, value_delimiter = ',')]
    tools: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    disallowed_tools: Vec<String>,
    #[arg(long)]
    disable_web_search: bool,
    #[arg(long)]
    no_memory: bool,
    #[arg(long)]
    pub verbatim: bool,
    #[arg(long)]
    system_prompt_override: Option<String>,
    #[arg(long)]
    rules: Option<String>,
    /// Permission preset. This does not provide operating-system confinement.
    #[arg(long, value_parser = ["readonly", "read-only", "workspace", "danger", "full"])]
    sandbox: Option<String>,
    #[arg(long, value_parser = ["default", "bypassPermissions", "yolo", "acceptEdits", "dontAsk"])]
    permission_mode: Option<String>,
    #[arg(long, alias = "always-approve")]
    dangerously_skip_permissions: bool,
    #[arg(long, value_delimiter = ',')]
    allow: Vec<String>,
    #[arg(long, value_delimiter = ',')]
    deny: Vec<String>,
}
impl Options {
    pub fn prepare(&self, config: &mut HarnessConfig, profile: &str) -> Result<(), String> {
        use PermissionMode::{Allow, Ask, Deny};
        let defaults = &mut config.permissions.defaults;
        let mut modes = match self.sandbox.as_deref() {
            Some("readonly" | "read-only") => Some((Deny, Deny, Deny)),
            Some("workspace") => Some((Ask, Ask, Deny)),
            Some("danger" | "full") => Some((Allow, Allow, Allow)),
            _ => None,
        };
        modes = match self.permission_mode.as_deref() {
            Some("default") => Some((Ask, Ask, Ask)),
            Some("acceptEdits") => Some((Allow, Ask, Ask)),
            Some("dontAsk") => Some((Deny, Deny, Deny)),
            Some("bypassPermissions" | "yolo") => Some((Allow, Allow, Allow)),
            _ => modes,
        };
        if self.dangerously_skip_permissions {
            modes = Some((Allow, Allow, Allow));
        }
        if self.dangerously_skip_permissions
            || matches!(
                self.permission_mode.as_deref(),
                Some("bypassPermissions" | "yolo")
            )
        {
            config.runtime.always_approve = true;
        }
        if let Some((edit, shell, network)) = modes {
            defaults.edit = edit;
            defaults.shell = shell;
            defaults.network = network;
        }
        for (tools, mode) in [(&self.allow, Allow), (&self.deny, Deny)] {
            for tool in tools {
                permission(defaults, tool, mode)?;
            }
        }
        if self.model.is_some() || self.variant.is_some() {
            let source = config
                .agents
                .get_mut(profile)
                .ok_or_else(|| format!("unknown profile: {profile}"))?;
            if let Some(model) = &self.model {
                source.model_ref.clone_from(model);
                source.model_ref_explicit = true;
            }
            if let Some(variant) = &self.variant {
                source.variant = Some(variant.clone());
            }
        }
        Ok(())
    }
    pub fn apply(&self, config: &mut CoordinatorConfig, selected: &str) -> Result<(), String> {
        for tool in self.tools.iter().chain(&self.disallowed_tools) {
            if config.tool_registry.get(tool).is_none() {
                return Err(format!("unknown tool: {tool}"));
            }
        }
        for profile in config.agent_profiles.values_mut() {
            if let Some(turns) = self.max_turns {
                profile.max_iters = Some(turns as usize);
            }
            profile.toolset.retain(|tool| {
                (self.tools.is_empty() || self.tools.contains(tool))
                    && !self.disallowed_tools.contains(tool)
                    && !(self.no_subagents && tool == "task")
                    && !(self.no_memory && tool == "memory")
                    && !(self.disable_web_search
                        && matches!(tool.as_str(), "websearch" | "webfetch" | "codesearch"))
            });
            if self.verbatim {
                continue;
            }
            if let Some(prompt) = &self.system_prompt_override {
                profile.system_prompt.clone_from(prompt);
                config.agent_prompt_sources.remove(&profile.name);
            }
            if let Some(rules) = &self.rules {
                profile.system_prompt.push_str("\n\n");
                profile.system_prompt.push_str(rules);
                if let Some(source) = config.agent_prompt_sources.get_mut(&profile.name) {
                    let source = std::sync::Arc::make_mut(source);
                    source.suffix.push_str("\n\n");
                    source.suffix.push_str(rules);
                }
            }
        }
        if let Some(target) = config.agent_model_targets.get_mut(selected) {
            if let Some(effort) = &self.reasoning_effort {
                target.reasoning_effort = Some(effort.clone());
            }
            if self.thinking
                || self.reasoning_effort.is_some()
                    && target.resolution.capabilities.supports_reasoning_summaries
            {
                target.reasoning_summary = Some("auto".into());
            }
        }
        config.session_mode_source = Some(harness_core::proj::SessionModeSource::Prompt);
        Ok(())
    }
    pub fn overrides_model(&self) -> bool {
        self.model.is_some()
            || self.variant.is_some()
            || self.thinking
            || self.reasoning_effort.is_some()
    }
    pub fn model_settings(&self) -> harness_core::agent::AgentModelSettings {
        harness_core::agent::AgentModelSettings {
            reasoning_effort: self.reasoning_effort.clone(),
            reasoning_summary: self.thinking.then(|| "auto".into()),
            ..Default::default()
        }
    }
}

fn permission(
    defaults: &mut PermissionDefaultsConfig,
    name: &str,
    mode: PermissionMode,
) -> Result<(), String> {
    let target = match name {
        "edit" | "write" | "patch" => {
            defaults.edit = mode;
            return Ok(());
        }
        "bash" | "shell" => {
            defaults.shell = mode;
            return Ok(());
        }
        "network" => {
            defaults.network = mode;
            return Ok(());
        }
        "question" => &mut defaults.question,
        "task" => &mut defaults.task,
        "read" => &mut defaults.read,
        "webfetch" => &mut defaults.webfetch,
        "websearch" => &mut defaults.websearch,
        "codesearch" => &mut defaults.codesearch,
        "lsp" => &mut defaults.lsp,
        "external" | "external_directory" => &mut defaults.external_directory,
        "doom" | "doom_loop" => &mut defaults.doom_loop,
        _ => return Err(format!("unknown permission kind: {name}")),
    };
    *target = Some(mode);
    Ok(())
}
