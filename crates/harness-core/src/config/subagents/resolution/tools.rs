use super::*;

pub(super) fn child_tools(
    config: &SubagentRuntimeConfig,
    context: &SubagentDefinitionContext<'_>,
    definition: &SubagentDefinition,
    mode: SubagentCapabilityMode,
    inherited_mcp: &[String],
) -> Vec<SubagentTool> {
    use SubagentToolKind as Kind;
    let mut tools: Vec<_> = context
        .tools
        .iter()
        .filter(|tool| {
            let declared = definition.tool_config.as_ref().map_or_else(
                || {
                    definition.declared_tools.is_empty()
                        || definition
                            .declared_tools
                            .iter()
                            .any(|name| tool_matches(name, tool))
                },
                |config| {
                    config
                        .tools
                        .iter()
                        .any(|declared| tool_matches(&declared.id, tool))
                },
            );
            let injected = definition.inject_default_tools
                && (matches!(
                    tool.kind,
                    Some(Kind::Read | Kind::WebSearch | Kind::WebFetch | Kind::Lsp)
                ) || tool.mcp_server.is_some());
            (declared || injected)
                && tool.kind.is_none_or(|kind| mode.allows(kind))
                // Eval can execute local code even without calling a host tool.
                && (tool.id != "eval" || mode.allows(Kind::Execute))
                && tool.kind != Some(Kind::AskUser)
                && tool.kind != Some(Kind::Feedback)
                && tool.kind != Some(Kind::Workflow)
                && (tool.mcp_server.is_some() || !child_stripped_id(&tool.id))
                && (tool.kind != Some(Kind::Task) || context.child_depth < config.max_depth)
                && (tool.kind != Some(Kind::ActiveAgentMessage) || config.messaging_enabled)
                && tool.mcp_server.as_ref().is_none_or(|server| {
                    inherited_mcp.contains(server) || definition.mcp_servers.contains(server)
                })
                && (definition.tools.is_empty()
                    || definition.tools.iter().any(|name| tool_matches(name, tool)))
                && !definition
                    .disallowed_tools
                    .iter()
                    .any(|name| tool_matches(name, tool))
                && context
                    .operator_allowed_tools
                    .is_none_or(|allowed| allowed.iter().any(|name| tool_matches(name, tool)))
                && !context
                    .operator_denied_tools
                    .iter()
                    .any(|name| tool_matches(name, tool))
        })
        .cloned()
        .map(|mut tool| {
            if let Some(declared) = definition.tool_config.as_ref().and_then(|config| {
                config
                    .tools
                    .iter()
                    .find(|declared| tool_matches(&declared.id, &tool))
            }) && declared
                .params
                .as_ref()
                .and_then(|params| params.get("enabled_background"))
                == Some(&serde_json::Value::Bool(false))
            {
                tool.background_capable = false;
            }
            tool
        })
        .collect();
    let has_background = tools.iter().any(|tool| {
        tool.kind == Some(Kind::Task) || tool.kind == Some(Kind::Execute) && tool.background_capable
    });
    if !has_background {
        tools.retain(|tool| {
            !matches!(
                tool.kind,
                Some(Kind::BackgroundTaskAction | Kind::KillTaskAction)
            )
        });
    }
    tools
}

fn child_stripped_id(id: &str) -> bool {
    let name = id.rsplit(':').next().unwrap_or(id);
    matches!(
        name,
        "workflow" | "ask_user_question" | "question" | "send_feedback" | "feedback"
    )
}

pub(super) fn definition_allowed_types(definition: &SubagentDefinition) -> Option<Vec<String>> {
    fn directive(value: &str) -> Option<&str> {
        let (name, types) = value.split_once('(')?;
        (name.eq_ignore_ascii_case("Agent") || name.eq_ignore_ascii_case("Task"))
            .then(|| types.strip_suffix(')'))
            .flatten()
    }
    if definition.disallowed_tools.iter().any(|tool| {
        tool.eq_ignore_ascii_case("Agent")
            || tool.eq_ignore_ascii_case("Task")
            || directive(tool) == Some("")
    }) {
        return Some(Vec::new());
    }
    let mut types: Vec<_> = definition
        .tools
        .iter()
        .filter_map(|tool| directive(tool))
        .flat_map(|types| types.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    let has_directive = definition.tools.iter().any(|tool| {
        tool.eq_ignore_ascii_case("Agent")
            || tool.eq_ignore_ascii_case("Task")
            || directive(tool).is_some()
    });
    if types.is_empty() {
        return (!has_directive && !definition.tools.is_empty()).then(Vec::new);
    }
    types.sort();
    types.dedup();
    let denied: Vec<_> = definition
        .disallowed_tools
        .iter()
        .filter_map(|tool| directive(tool))
        .flat_map(|types| types.split(','))
        .map(str::trim)
        .collect();
    types.retain(|name| {
        !denied
            .iter()
            .any(|denied| name.eq_ignore_ascii_case(denied))
    });
    Some(types)
}

fn tool_matches(name: &str, tool: &SubagentTool) -> bool {
    let short = tool.id.rsplit(':').next().unwrap_or(&tool.id);
    name.eq_ignore_ascii_case(&tool.id)
        || name.eq_ignore_ascii_case(short)
        || match tool.kind {
            Some(SubagentToolKind::Read) => matches!(name, "Read" | "ReadFile" | "read"),
            Some(SubagentToolKind::ListDir | SubagentToolKind::List) => {
                matches!(name, "ListDir" | "List" | "list")
            }
            Some(SubagentToolKind::Search) => matches!(name, "Grep" | "Search" | "grep"),
            Some(SubagentToolKind::Plan) => matches!(name, "TodoWrite" | "todowrite"),
            Some(SubagentToolKind::Execute) => matches!(name, "Bash" | "bash" | "Shell"),
            Some(SubagentToolKind::Task) => {
                name.eq_ignore_ascii_case("Agent")
                    || name.eq_ignore_ascii_case("Task")
                    || name.split_once('(').is_some_and(|(name, types)| {
                        (name.eq_ignore_ascii_case("Agent") || name.eq_ignore_ascii_case("Task"))
                            && types.ends_with(')')
                    })
            }
            _ => false,
        }
}
