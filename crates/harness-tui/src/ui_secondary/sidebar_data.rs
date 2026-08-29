// allow: SIZE_OK — TUI rendering (indivisible view model)
use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use crate::app::task_child_session_id_from_output;
use crate::text::{
    has_trimmed_content, trimmed_json_nested_string_field, trimmed_json_string_field,
};

pub(super) fn build_operator_rail_model(app: &AppState) -> OperatorRailModel {
    let mut sections = Vec::new();
    let subagent_groups = operator_sidebar_subagent_groups(app);
    if !subagent_groups.is_empty()
        && app.overlay_stack().top() != Some(crate::overlay::OverlayKind::SubagentActions)
    {
        sections.push(OperatorRailBodySection::Subagents {
            groups: subagent_groups,
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::Subagents,
                collapsed: app
                    .operator_sidebar_section_collapsed(OperatorSidebarSection::Subagents),
            },
        });
    }
    let todo_items = operator_sidebar_todo_items(app);
    if let Some(items) = todo_items {
        let disclosure = (items.len() > 2).then(|| OperatorRailSectionDisclosure {
            section: OperatorSidebarSection::Todo,
            collapsed: app.operator_sidebar_section_collapsed(OperatorSidebarSection::Todo),
        });
        sections.push(OperatorRailBodySection::Todo { items, disclosure });
    }
    sections.extend([
        OperatorRailBodySection::Mcp {
            items: operator_sidebar_mcp_items(app),
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::Mcp,
                collapsed: app.operator_sidebar_section_collapsed(OperatorSidebarSection::Mcp),
            },
        },
        OperatorRailBodySection::Lsp {
            items: operator_sidebar_lsp_items(app),
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::Lsp,
                collapsed: app.operator_sidebar_section_collapsed(OperatorSidebarSection::Lsp),
            },
        },
        OperatorRailBodySection::ModifiedFiles {
            items: operator_sidebar_modified_file_rows(app),
            disclosure: OperatorRailSectionDisclosure {
                section: OperatorSidebarSection::ModifiedFiles,
                collapsed: app
                    .operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles),
            },
        },
    ]);

    OperatorRailModel {
        title: operator_sidebar_session_title(app),
        body: OperatorRailBody { sections },
    }
}

fn operator_sidebar_session_title(app: &AppState) -> Option<OperatorRailTitle> {
    if let Some(title) = app
        .events
        .iter()
        .rev()
        .find_map(|event| match &event.payload {
            harness_core::event::EventV1::SessionTitleUpdated(data) => {
                Some(sanitize_operator_sidebar_line(&data.title))
            }
            _ => None,
        })
    {
        if !title.is_empty() {
            return Some(OperatorRailTitle::Generated(title));
        }
    }

    let user_title = app
        .activities
        .iter()
        .find_map(|activity| activity.user_message.as_ref())
        .map(|message| sanitize_operator_sidebar_line(&message.text))
        .filter(|text| !text.is_empty());

    let prompt_submitted = app
        .activities
        .iter()
        .any(|activity| activity.user_message.is_some());
    let provider_started = app.events.iter().any(|event| {
        matches!(
            event.payload,
            harness_core::event::EventV1::ProviderRequestStarted(_)
        )
    });

    if !app.replay_mode && prompt_submitted && !provider_started {
        return Some(OperatorRailTitle::Pending);
    }

    if let Some(title) = user_title {
        return Some(OperatorRailTitle::Generated(title));
    }

    None
}

fn operator_sidebar_todo_items(app: &AppState) -> Option<Vec<OperatorRailItem>> {
    let todos = app
        .activities
        .iter()
        .flat_map(|activity| activity.tool_calls.iter())
        .rev()
        .find_map(|tool_call| todo_items_from_tool_call(tool_call, app.session_path.as_deref()))?;

    let has_open_todo = todos.iter().any(|item| {
        matches!(
            item,
            OperatorRailItem::Todo { status, .. } if *status != TodoRailStatus::Completed
        )
    });
    (has_open_todo && !todos.is_empty()).then_some(todos)
}

fn operator_sidebar_subagent_groups(app: &AppState) -> Vec<SubagentRailGroup> {
    let mut groups: Vec<SubagentRailGroup> = Vec::new();
    let mut parent_tool_call_ids = std::collections::BTreeSet::new();
    let mut child_session_ids = std::collections::BTreeSet::new();
    let mut child_request_ids = std::collections::BTreeSet::new();
    let mut unlinked_active_tool_counts: BTreeMap<String, usize> = BTreeMap::new();
    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            if !operator_sidebar_tool_call_is_task_spawn(tool_call) {
                continue;
            }
            let Some(agent_name) = subagent_agent_name_from_tool_call(tool_call) else {
                continue;
            };
            let child_session_id = subagent_child_session_id(tool_call);
            parent_tool_call_ids.insert(tool_call.tool_call_id.clone());
            if let Some(child_session_id) = child_session_id.as_ref() {
                child_session_ids.insert(child_session_id.clone());
            }
            let child_request_id = subagent_child_request_id(tool_call);
            if let Some(child_request_id) = child_request_id.as_ref() {
                child_request_ids.insert(child_request_id.clone());
            }
            let status = subagent_status_from_app(
                app,
                tool_call,
                child_session_id.as_deref(),
                child_request_id.as_deref(),
            );
            if child_session_id.is_none() && child_request_id.is_none() && status.is_active() {
                *unlinked_active_tool_counts
                    .entry(agent_name.clone())
                    .or_default() += 1;
            }
            let item = SubagentRailItem {
                description: String::new(),
                status,
                child_session_id,
            };
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.agent_name == agent_name)
            {
                group.items.push(item);
            } else {
                groups.push(SubagentRailGroup {
                    expanded: app.operator_sidebar_subagent_group_expanded(&agent_name),
                    agent_name,
                    items: vec![item],
                });
            }
        }
    }

    for row in app.orchestration_visible_rows() {
        if app.replay_mode && background_notification_for_orchestration_row(app, &row).is_none() {
            continue;
        }
        if row
            .parent_tool_call_id
            .as_ref()
            .is_some_and(|id| parent_tool_call_ids.contains(id))
        {
            continue;
        }
        if row
            .effective_child_session_id()
            .is_some_and(|id| child_session_ids.contains(id))
        {
            continue;
        }
        if row
            .effective_child_request_id()
            .is_some_and(|id| child_request_ids.contains(id))
        {
            continue;
        }
        let Some(agent_name) = row
            .queue_key
            .as_deref()
            .and_then(subagent_name_from_queue_key)
            .or_else(|| subagent_name_from_background_notification_row(app, &row))
            .or_else(|| subagent_name_from_child_orchestration_row(app, &row))
        else {
            continue;
        };
        let status = SubagentRailStatus::from_orchestration_state(row.state);
        if row.parent_tool_call_id.is_none() && status.is_active() {
            if let Some(count) = unlinked_active_tool_counts.get_mut(&agent_name) {
                if *count > 0 {
                    *count -= 1;
                    continue;
                }
            }
        }
        let item = SubagentRailItem {
            description: String::new(),
            status,
            child_session_id: row.effective_child_session_id().map(str::to_string),
        };
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.agent_name == agent_name)
        {
            group.items.push(item);
        } else {
            groups.push(SubagentRailGroup {
                expanded: app.operator_sidebar_subagent_group_expanded(&agent_name),
                agent_name,
                items: vec![item],
            });
        }
    }

    assign_subagent_task_labels(&mut groups);
    groups
}

fn assign_subagent_task_labels(groups: &mut [SubagentRailGroup]) {
    for group in groups {
        let base = format!("{} Task", group.agent_name);
        let multiple = group.items.len() > 1;
        for (index, item) in group.items.iter_mut().enumerate() {
            item.description = if multiple && index > 0 {
                format!("{base} {}", index + 1)
            } else {
                base.clone()
            };
        }
    }
}

fn subagent_name_from_queue_key(queue_key: &str) -> Option<String> {
    ["agent:queue:", "agent:queued:", "agent:running:"]
        .iter()
        .find_map(|prefix| queue_key.strip_prefix(prefix))
        .and_then(subagent_agent_label)
}

fn subagent_name_from_background_notification_row(
    app: &AppState,
    row: &crate::app::OrchestrationTaskRow,
) -> Option<String> {
    let notification = background_notification_for_orchestration_row(app, row)?;
    let agent_name = subagent_profile_for_agent_id(app, notification.child_session_id.as_str())
        .or_else(|| subagent_profile_for_agent_id(app, notification.task_id.as_str()))
        .unwrap_or_else(|| "Subagent".to_string());
    non_empty_sanitized_operator_sidebar_line(&agent_name)
}

fn subagent_name_from_child_orchestration_row(
    app: &AppState,
    row: &crate::app::OrchestrationTaskRow,
) -> Option<String> {
    let child_session_id = row.effective_child_session_id()?;
    child_subagent_profile_for_agent_id(app, child_session_id).or_else(|| {
        row.owner_agent_id
            .as_deref()
            .and_then(|id| child_subagent_profile_for_agent_id(app, id))
    })
}

fn background_notification_for_orchestration_row<'a>(
    app: &'a AppState,
    row: &crate::app::OrchestrationTaskRow,
) -> Option<&'a harness_core::event::BackgroundTaskNotificationEvent> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::BackgroundTaskNotification(data) = &event.payload else {
            return None;
        };
        (data.task_id.as_str() == row.task_id.as_str()
            || row.effective_child_request_id() == Some(data.child_request_id.as_str())
            || row.effective_child_session_id() == Some(data.child_session_id.as_str()))
        .then_some(data)
    })
}

fn subagent_profile_for_agent_id(app: &AppState, agent_id: &str) -> Option<String> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::AgentSpawned(data) = &event.payload else {
            return None;
        };
        (data.agent_id == agent_id)
            .then(|| subagent_agent_label(&data.profile))
            .flatten()
    })
}

fn child_subagent_profile_for_agent_id(app: &AppState, agent_id: &str) -> Option<String> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::AgentSpawned(data) = &event.payload else {
            return None;
        };
        (data.agent_id == agent_id && data.parent_agent_id.is_some())
            .then(|| subagent_agent_label(&data.profile))
            .flatten()
    })
}

fn subagent_agent_label(value: &str) -> Option<String> {
    let sanitized = sanitize_operator_sidebar_line(value);
    if sanitized.is_empty() {
        return None;
    }
    Some(crate::app::humanize_profile_label(&sanitized))
}

fn non_empty_sanitized_operator_sidebar_line(text: &str) -> Option<String> {
    let sanitized = sanitize_operator_sidebar_line(text);
    (!sanitized.is_empty()).then_some(sanitized)
}

fn subagent_status_from_app(
    app: &AppState,
    tool_call: &crate::app::ToolCallEntry,
    child_session_id: Option<&str>,
    child_request_id: Option<&str>,
) -> SubagentRailStatus {
    if let Some(status) =
        subagent_status_from_background_notification(app, child_session_id, child_request_id)
    {
        return status;
    }

    if let Some(row) = app.orchestration_visible_rows().into_iter().find(|row| {
        row.parent_tool_call_id.as_deref() == Some(tool_call.tool_call_id.as_str())
            || child_session_id.is_some_and(|child| {
                row.effective_child_session_id() == Some(child) || row.task_id == child
            })
            || child_request_id.is_some_and(|child| row.effective_child_request_id() == Some(child))
    }) {
        return SubagentRailStatus::from_orchestration_state(row.state);
    }

    if let Some(status) = subagent_status_from_output_json(tool_call.output_json.as_ref()) {
        return status;
    }

    SubagentRailStatus::from_tool_call_status(tool_call.status)
}

fn subagent_status_from_output_json(
    output_json: Option<&serde_json::Value>,
) -> Option<SubagentRailStatus> {
    let status = trimmed_json_string_field(output_json, &["status", "final_status"])?;
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" => Some(SubagentRailStatus::Queued),
        "scheduled" | "running" | "in_progress" => Some(SubagentRailStatus::Running),
        "completed" | "succeeded" | "success" => Some(SubagentRailStatus::Completed),
        "cancelled" | "failed" | "timed_out" | "error" => Some(SubagentRailStatus::Error),
        _ => None,
    }
}

fn subagent_status_from_background_notification(
    app: &AppState,
    child_session_id: Option<&str>,
    child_request_id: Option<&str>,
) -> Option<SubagentRailStatus> {
    app.events.iter().rev().find_map(|event| {
        let harness_core::event::EventV1::BackgroundTaskNotification(data) = &event.payload else {
            return None;
        };
        let matches_child = child_request_id == Some(data.child_request_id.as_str())
            || child_session_id == Some(data.child_session_id.as_str())
            || child_session_id == Some(data.task_id.as_str());
        matches_child.then_some(match data.status {
            harness_core::event::BackgroundTaskNotificationStatus::Completed => {
                SubagentRailStatus::Completed
            }
            harness_core::event::BackgroundTaskNotificationStatus::Cancelled
            | harness_core::event::BackgroundTaskNotificationStatus::Failed
            | harness_core::event::BackgroundTaskNotificationStatus::TimedOut => {
                SubagentRailStatus::Error
            }
        })
    })
}

fn operator_sidebar_tool_call_is_task_spawn(tool_call: &crate::app::ToolCallEntry) -> bool {
    matches!(tool_call.effective_tool_id(), "agent.spawn" | "task")
        || matches!(tool_call.tool_id.as_str(), "agent.spawn" | "task")
}

fn subagent_agent_name_from_tool_call(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    let args = serde_json::from_str::<serde_json::Value>(&tool_call.args_summary).ok();
    let agent_name = trimmed_json_string_field(
        tool_call.output_json.as_ref(),
        &["profile", "profile_name", "subagent_type", "category"],
    )
    .or_else(|| {
        trimmed_json_nested_string_field(
            tool_call.output_json.as_ref(),
            &["route", "resolved_profile"],
        )
    })
    .or_else(|| {
        trimmed_json_nested_string_field(tool_call.output_json.as_ref(), &["route", "profile_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(
            tool_call.output_json.as_ref(),
            &["route", "requested_category"],
        )
    })
    .or_else(|| {
        trimmed_json_string_field(
            args.as_ref(),
            &["subagent_type", "profile", "profile_name", "category"],
        )
    })?;
    subagent_agent_label(&agent_name)
}

fn subagent_child_session_id(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_session_id.clone())
        .or_else(|| task_child_session_id_from_output(tool_call.output_json.as_ref()))
}

fn subagent_child_request_id(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    tool_call
        .lineage
        .as_ref()
        .and_then(|lineage| lineage.child_request_id.clone())
        .or_else(|| subagent_child_request_id_from_output(tool_call.output_json.as_ref()))
}

fn subagent_child_request_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<String> {
    trimmed_json_string_field(
        output_json,
        &["child_request_id", "request_id", "requestId"],
    )
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "child_request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "request_id"]))
    .or_else(|| trimmed_json_nested_string_field(output_json, &["lineage", "requestId"]))
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "child_request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "request_id"])
    })
    .or_else(|| {
        trimmed_json_nested_string_field(output_json, &["_harness", "lineage", "requestId"])
    })
}

fn todo_items_from_tool_call(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<Vec<OperatorRailItem>> {
    if !matches!(
        tool_call.effective_tool_id(),
        "todo.write" | "todo.read" | "todowrite" | "todoread"
    ) && !matches!(tool_call.tool_id.as_str(), "todowrite" | "todoread")
    {
        return None;
    }
    if tool_call.status != crate::app::ToolCallDisplayStatus::Succeeded {
        return None;
    }

    let artifact_json = || todo_json_from_artifacts(tool_call, session_path);
    let todos = tool_call
        .output_json
        .as_ref()
        .and_then(todo_array_from_json)
        .cloned()
        .or_else(artifact_json)?;
    Some(
        todos
            .iter()
            .filter_map(|todo| {
                let content = todo.get("content")?.as_str()?;
                let content = sanitize_operator_sidebar_line(content);
                (!content.is_empty()).then(|| OperatorRailItem::Todo {
                    content,
                    status: todo
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        .map(TodoRailStatus::from_value)
                        .unwrap_or(TodoRailStatus::Pending),
                })
            })
            .collect(),
    )
}

fn todo_array_from_json(value: &serde_json::Value) -> Option<&Vec<serde_json::Value>> {
    value
        .get("todos")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .or_else(|| {
            value
                .get("structured_output")
                .and_then(todo_array_from_json)
        })
}

fn todo_json_from_artifacts(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<Vec<serde_json::Value>> {
    let session_path = session_path?;
    tool_call.artifact_refs.iter().find_map(|artifact| {
        if !(artifact.path.ends_with(".json") || artifact.path.ends_with(".txt")) {
            return None;
        }
        let path = Path::new(&artifact.path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| component == std::path::Component::ParentDir)
        {
            return None;
        }
        let contents = std::fs::read_to_string(session_path.join(path)).ok()?;
        let json = serde_json::from_str::<serde_json::Value>(&contents).ok()?;
        todo_array_from_json(&json).cloned()
    })
}

pub(super) fn operator_sidebar_mcp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let mut items = BTreeMap::new();
    if let Some(integrations) = harness_core::config::registered_integrations_config() {
        if has_trimmed_content(&integrations.remote_search.endpoint) {
            items.insert("websearch".to_string(), RuntimeHealthState::Unhealthy);
        }
        for (name, server) in &integrations.mcp.servers {
            if !server.enabled() {
                continue;
            }
            let state = match harness_core::config::registered_mcp_server_connection_state(name) {
                Some(harness_core::config::McpServerConnectionState::Connected) => {
                    RuntimeHealthState::Healthy
                }
                Some(harness_core::config::McpServerConnectionState::Failed(_)) => {
                    RuntimeHealthState::Unhealthy
                }
                None => RuntimeHealthState::Unhealthy,
            };
            items.insert(sanitize_operator_sidebar_line(name), state);
        }
    }

    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            let Some(server_name) = runtime_mcp_server_name(tool_call) else {
                continue;
            };
            let Some(state) = runtime_health_state_for_tool_call(tool_call) else {
                continue;
            };
            items
                .entry(server_name)
                .and_modify(|entry| *entry = state)
                .or_insert(state);
        }
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain(
            "No MCP servers configured".to_string(),
        )]
    } else {
        items
            .into_iter()
            .map(|(name, state)| OperatorRailItem::Status {
                label: name,
                suffix: Some(
                    match state {
                        RuntimeHealthState::Healthy => "Connected",
                        RuntimeHealthState::Unhealthy => "Disconnected",
                    }
                    .to_string(),
                ),
                state,
            })
            .collect()
    }
}

pub(super) fn operator_sidebar_lsp_items(app: &AppState) -> Vec<OperatorRailItem> {
    let config = harness_core::config::registered_lsp_config();
    if config.disabled {
        return vec![OperatorRailItem::Plain("LSP is disabled".to_string())];
    }

    let mut items = BTreeMap::new();
    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            let Some(server_name) = runtime_lsp_server_name(tool_call) else {
                continue;
            };
            items.insert(server_name, RuntimeHealthState::Healthy);
        }
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain("No active LSP servers".to_string())]
    } else {
        items
            .into_iter()
            .map(|(name, state)| OperatorRailItem::Status {
                label: name,
                suffix: None,
                state,
            })
            .collect()
    }
}

fn runtime_health_state_for_tool_call(
    tool_call: &crate::app::ToolCallEntry,
) -> Option<RuntimeHealthState> {
    match tool_call.status {
        crate::app::ToolCallDisplayStatus::Succeeded
        | crate::app::ToolCallDisplayStatus::Running => Some(RuntimeHealthState::Healthy),
        crate::app::ToolCallDisplayStatus::Failed => Some(RuntimeHealthState::Unhealthy),
        crate::app::ToolCallDisplayStatus::PendingPermission
        | crate::app::ToolCallDisplayStatus::Queued => None,
    }
}

fn runtime_mcp_server_name(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    match tool_call.effective_tool_id() {
        canonical if canonical.starts_with("mcp.") => canonical
            .split('.')
            .nth(1)
            .map(sanitize_operator_sidebar_line)
            .filter(|name| !name.is_empty()),
        "search.web" | "search.code" => Some("websearch".to_string()),
        _ => None,
    }
}

fn runtime_lsp_server_name(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    match tool_call.effective_tool_id() {
        "lsp" | "lsp.rename" | "code.lsp" | "code.lsp.rename" => {
            trimmed_json_nested_string_field(tool_call.output_json.as_ref(), &["server", "name"])
                .or_else(|| ui_lsp::server_name_from_args(&tool_call.args_summary))
                .map(|name| sanitize_operator_sidebar_line(&name))
                .filter(|name| !name.is_empty())
        }
        _ => None,
    }
}

fn operator_sidebar_modified_file_rows(app: &AppState) -> Vec<OperatorRailItem> {
    let mut seen = std::collections::BTreeSet::new();
    let mut items = Vec::new();

    for event in app.events.iter().rev() {
        let harness_core::event::EventV1::EditApplied(edit) = &event.payload else {
            continue;
        };
        if !seen.insert(edit.path.clone()) {
            continue;
        }

        let path = sanitize_operator_sidebar_line(&edit.path);
        let counts = edit.diff_rel_path.as_deref().and_then(|diff_rel_path| {
            app.session_path.as_ref().and_then(|session_path| {
                std::fs::read_to_string(session_path.join(diff_rel_path))
                    .ok()
                    .and_then(|diff_content| {
                        super::ui_diff::structured_diff_stats(&diff_content, Some(&edit.path), true)
                    })
            })
        });

        let (additions, removals) = counts
            .map(|(additions, removals)| (Some(additions), Some(removals)))
            .unwrap_or((None, None));
        items.push(OperatorRailItem::ModifiedFile {
            path,
            additions,
            removals,
        });
    }

    if items.is_empty() {
        vec![OperatorRailItem::Plain("No files modified".to_string())]
    } else {
        items
    }
}

pub(super) fn activity_surface_visible(app: &AppState) -> bool {
    (app.replay_mode && app.active_tab == Tab::Run)
        || (!app.replay_mode && app.review_surface().is_none())
}
