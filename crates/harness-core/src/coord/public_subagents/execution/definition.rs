use super::*;

mod admission;

impl Runtime {
    /// Pure admission selector used by generic tool permission handling. A
    /// wildcard declaration is not an approval for the resolved definition.
    pub(in crate::coord) fn native_spawn_selector(
        &self,
        actor: &EventActor,
        args: &Value,
    ) -> Result<String, CoordinatorError> {
        let input: SpawnSubagentInput = serde_json::from_value(args.clone())?;
        self.resolve_native_definition(actor, &input)
            .map(|(resolved, _, _)| resolved.subagent_type)
    }

    fn native_definitions(
        &self,
        cwd: &Path,
    ) -> Result<SubagentDefinitionSnapshot, CoordinatorError> {
        if let Some(discovery) = &self.config.subagent_discovery {
            let mut discovery = discovery.clone();
            discovery.cwd = cwd.into();
            return Ok(discover_subagent_definitions(
                &self.config.subagents,
                &discovery,
            ));
        }
        let mut definitions = self.config.subagent_definitions.clone().unwrap_or_default();
        definitions
            .roles
            .extend(self.config.subagents.roles.clone());
        definitions
            .personas
            .extend(self.config.subagents.personas.clone());
        Ok(definitions)
    }

    fn native_tools(&self, parent: &str) -> Vec<SubagentTool> {
        self.config
            .tool_registry
            .tool_ids_for(self.tool_scope(Some(parent)).as_deref())
            .into_iter()
            .map(|id| {
                let kind = match id.as_str() {
                    "read_file" | "read" => Some(SubagentToolKind::Read),
                    "list" | "list_dir" | "glob" => Some(SubagentToolKind::ListDir),
                    "grep" | "search" => Some(SubagentToolKind::Search),
                    "lsp"
                    | "lsp_diagnostics"
                    | "lsp_symbols"
                    | "lsp_goto_definition"
                    | "lsp_find_references"
                    | "lsp_prepare_rename"
                    | "lsp_rename" => Some(SubagentToolKind::Lsp),
                    "todo" | "todo_write" | "todowrite" => Some(SubagentToolKind::Plan),
                    "web_search" => Some(SubagentToolKind::WebSearch),
                    "webfetch" | "web_fetch" => Some(SubagentToolKind::WebFetch),
                    "spawn_subagent" | "task" => Some(SubagentToolKind::Task),
                    "get_command_or_subagent_output"
                    | "wait_commands_or_subagents"
                    | "get_task_output"
                    | "wait_tasks" => Some(SubagentToolKind::BackgroundTaskAction),
                    "kill_command_or_subagent" | "kill_task" => {
                        Some(SubagentToolKind::KillTaskAction)
                    }
                    "send_subagent_message" => Some(SubagentToolKind::ActiveAgentMessage),
                    "question" | "ask_user" | "ask_user_question" => {
                        Some(SubagentToolKind::AskUser)
                    }
                    "feedback" => Some(SubagentToolKind::Feedback),
                    "skill" => Some(SubagentToolKind::Skill),
                    "memory_search" => Some(SubagentToolKind::MemorySearch),
                    "memory_get" => Some(SubagentToolKind::MemoryGet),
                    "write_file" | "write" => Some(SubagentToolKind::Write),
                    "delete_file" => Some(SubagentToolKind::Delete),
                    "move_file" => Some(SubagentToolKind::Move),
                    "edit_file" | "edit" | "apply_patch" => Some(SubagentToolKind::Edit),
                    "bash" | "shell" => Some(SubagentToolKind::Execute),
                    _ => None,
                };
                let mcp_server = id
                    .strip_prefix("mcp__")
                    .and_then(|rest| rest.split_once("__"))
                    .or_else(|| {
                        id.strip_prefix("mcp.")
                            .and_then(|rest| rest.split_once('.'))
                    })
                    .map(|(server, _)| server.to_owned());
                let kind = if mcp_server.is_some() {
                    kind
                } else {
                    match self
                        .config
                        .tool_registry
                        .get_for(&id, self.tool_scope(Some(parent)).as_deref())
                        .map(|tool| tool.capability())
                    {
                        Some(ToolCapability::EditFs) => match kind {
                            Some(
                                SubagentToolKind::Write
                                | SubagentToolKind::Edit
                                | SubagentToolKind::Delete
                                | SubagentToolKind::Move,
                            ) => kind,
                            _ => Some(SubagentToolKind::Edit),
                        },
                        Some(ToolCapability::Shell) => Some(SubagentToolKind::Execute),
                        _ => kind,
                    }
                };
                SubagentTool {
                    background_capable: matches!(kind, Some(SubagentToolKind::Execute)),
                    id,
                    kind,
                    mcp_server,
                }
            })
            .collect()
    }

    fn resolve_native_definition(
        &self,
        actor: &EventActor,
        input: &SpawnSubagentInput,
    ) -> Result<
        (
            ResolvedSubagentDefinition,
            Option<Box<RawFinalizedState>>,
            Option<FinalizedAgentStateReferenceV1>,
        ),
        CoordinatorError,
    > {
        self.accepting()?;
        let parent_id = actor.agent_id.as_deref().ok_or_else(|| {
            CoordinatorError::PermissionDenied("subagent spawn requires a parent agent".into())
        })?;
        let parent = self
            .agents
            .get(parent_id)
            .ok_or_else(|| CoordinatorError::UnknownAgent(parent_id.into()))?;
        let depth = self.native_depth(parent_id);
        if depth >= self.config.subagents.max_depth {
            return Err(native_invalid(format!(
                "Subagent depth limit exceeded (current depth: {depth}, max: {}). Cannot spawn further nested subagents.",
                self.config.subagents.max_depth
            )));
        }
        if let Some(injected) = input.task_id.as_deref() {
            let _ = native_id(Some(injected))?;
        }
        let resume = optional(input.resume_from.as_deref());
        let source = resume.as_deref().map(|source| {
            let native = self.native_subagents.get(source);
            if native.is_some_and(|child| child.phase != NativePhase::Terminal) {
                return Err(native_invalid(format!(
                    "Cannot resume from subagent '{source}': it is still running. Wait for it to complete before resuming."
                )));
            }
            let source_agent = self.agents.get(source).ok_or_else(|| native_invalid(format!(
                "Cannot resume from subagent '{source}': not found. The subagent may have been evicted or the ID is invalid."
            )))?;
            let root = self.native_root(parent_id)?;
            let source_root = self.native_root(source)?;
            if source_root != root {
                return Err(CoordinatorError::PermissionDenied("resume source belongs to another parent session".into()));
            }
            let FinalizedStateResult::Available { state } = self.resolve_agent_finalized_state(source)? else {
                return Err(native_invalid(format!(
                    "Cannot resume from subagent '{source}': persisted session state is unavailable."
                )));
            };
            Ok((state, source_agent.finalized.clone(), native.map(|n| n.registration.subagent_type.clone())
                .unwrap_or_else(|| source_agent.profile.name.clone())))
        }).transpose()?;
        let allowed = self
            .native_subagents
            .get(parent_id)
            .and_then(|child| child.registration.allowed_types.as_deref());
        let tools = self.native_tools(parent_id);
        let mcp: Vec<_> = tools.iter().filter_map(|t| t.mcp_server.clone()).collect();
        let definitions = self.native_definitions(&parent.cwd)?;
        let policy = self
            .native_model_policies
            .get(parent_id)
            .cloned()
            .unwrap_or_else(|| {
                SubagentModelPolicy::latch(
                    self.config.subagents.model_inheritance,
                    &self
                        .config
                        .subagent_model_catalog
                        .clone()
                        .unwrap_or_default(),
                    None,
                )
            });
        let context = SubagentDefinitionContext {
            cwd: &parent.cwd,
            definitions: &definitions,
            parent_model: &parent.info.model_ref,
            parent_reasoning_effort: parent.settings.reasoning_effort.as_deref(),
            parent_variant: parent
                .target
                .as_ref()
                .and_then(|target| target.variant.as_deref()),
            parent_max_turns: parent
                .profile
                .max_iters
                .and_then(|n| u32::try_from(n).ok())
                .and_then(std::num::NonZeroU32::new),
            allowed_types: allowed,
            catalog: self.config.subagent_model_catalog.as_ref(),
            model_selection: policy.selection,
            tools: &tools,
            operator_allowed_tools: (!parent.profile.toolset.iter().any(|t| t == "*"))
                .then_some(parent.profile.toolset.as_slice()),
            operator_denied_tools: &[],
            parent_permission_mode: None,
            managed_block_bypass: false,
            child_depth: depth.saturating_add(1).max(1),
            parent_mcp_servers: &mcp,
            parent_skills: &[],
        };
        let request = SubagentDefinitionRequest {
            subagent_type: source
                .as_ref()
                .map(|s| s.2.clone())
                .unwrap_or_else(|| input.subagent_type.clone()),
            type_specified: input.subagent_type_specified || source.is_some(),
            resume: source.is_some(),
            model: if source.is_some() {
                None
            } else {
                optional(input.model.as_deref())
            },
            variant: if source.is_some() {
                None
            } else {
                optional(input.variant.as_deref())
            },
            isolation: input.isolation,
            ..Default::default()
        };
        let mut resolved = resolve_subagent_definition(&self.config.subagents, &request, &context)
            .map_err(native_resolution_error)?;
        if let Some(tools) = &input.tools {
            if tools.len() > 256 || tools.iter().any(|id| id.is_empty() || id.len() > 256) {
                return Err(native_invalid("invalid child tool restriction".into()));
            }
            for tool in self
                .config
                .tool_registry
                .scoped_tools(&self.tool_scope(Some(parent_id)).unwrap_or_default())
            {
                if tools.iter().any(|name| name == tool.id())
                    && !resolved.tools.iter().any(|entry| entry.id == tool.id())
                {
                    resolved.tools.push(SubagentTool {
                        id: tool.id().into(),
                        kind: None,
                        mcp_server: None,
                        background_capable: false,
                    });
                }
            }
            resolved.tools.retain(|tool| tools.contains(&tool.id));
        }
        let (state, reference) = source.map_or((None, None), |(state, reference, _)| {
            (Some(state), reference)
        });
        if let Some(state) = &state {
            if !self
                .config
                .subagent_model_catalog
                .as_ref()
                .is_some_and(|catalog| {
                    catalog.contains(&state.source_model)
                        || catalog.contains(
                            &crate::agent::AgentModelRef::parse(&state.source_model).model_id,
                        )
                })
            {
                return Err(native_invalid(format!(
                    "Cannot resume from subagent '{}': source model '{}' is no longer available in the model catalogue.",
                    resume.as_deref().unwrap_or_default(), state.source_model
                )));
            }
            resolved.model.clone_from(&state.source_model);
        } else if let Some(target) = self.native_model_target(&resolved.model) {
            resolved.model = target.model_ref;
        }
        Ok((resolved, state, reference))
    }

    pub(in crate::coord) fn native_subagent_schema(
        &self,
        agent: &str,
    ) -> Result<Value, CoordinatorError> {
        let state = self
            .agents
            .get(agent)
            .ok_or_else(|| CoordinatorError::UnknownAgent(agent.into()))?;
        let definitions = self.native_definitions(&state.cwd)?;
        let allowed = self
            .native_subagents
            .get(agent)
            .and_then(|child| child.registration.allowed_types.as_deref());
        let types = definitions.selectable_types(&self.config.subagents.toggle, allowed);
        let selection = self
            .native_model_policies
            .get(agent)
            .map_or(SubagentModelSelection::Selectable, |policy| {
                policy.selection
            });
        Ok(serde_json::json!({
            "subagent_type": subagent_type_schema(&types),
            "model_selectable": selection == SubagentModelSelection::Selectable,
            "depth": self.native_depth(agent),
            "max_depth": self.config.subagents.max_depth,
            "allowed_types": allowed,
        }))
    }
}

fn native_resolution_error(error: SubagentResolutionError) -> CoordinatorError {
    if error == SubagentResolutionError::ValidationUnavailable {
        return CoordinatorError::Native {
            code: "validation_unavailable".into(),
            message: error.to_string(),
        };
    }
    let text = match error {
        SubagentResolutionError::Unknown { name, available } => format!(
            "Unknown subagent type: {name}{}",
            if available.is_empty() {
                String::new()
            } else {
                format!(". Available types: {}", available.join(", "))
            }
        ),
        SubagentResolutionError::NotAllowed { name, allowed } => format!(
            "agent can only spawn: {}; '{name}' not allowed",
            allowed.join(", ")
        ),
        SubagentResolutionError::Disabled { name } => {
            format!("Subagent '{name}' is disabled via [subagents.toggle] in config.toml")
        }
        error => error.to_string(),
    };
    native_invalid(text)
}
