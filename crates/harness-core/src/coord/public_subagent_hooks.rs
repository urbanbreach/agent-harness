use super::{context::Context, runtime::*, *};
use harness_providers::{MessageRole, ToolDef};

impl CoordinatorHandle {
    pub(super) async fn capture_active_context(
        &self,
        actor: EventActor,
        request: String,
        mut context: Context,
    ) -> Result<(), CoordinatorError> {
        let length = context.model_request.as_ref().map(|request| {
            request
                .messages
                .iter()
                .filter(|message| message.role != MessageRole::System)
                .count()
        });
        context
            .entries
            .retain(|entry| entry.message.role != MessageRole::System);
        if let Some(length) = length {
            context.entries.truncate(length);
        }
        self.call(move |runtime| {
            let job = runtime
                .running
                .get(&request)
                .ok_or_else(|| CoordinatorError::UnknownTask(request.clone()))?;
            if job.actor.agent_id != actor.agent_id || !matches!(job.kind, JobKind::Turn { .. }) {
                return Err(CoordinatorError::PermissionDenied(
                    "logical context requires its worker".into(),
                ));
            }
            let agent = actor.agent_id.ok_or_else(|| {
                CoordinatorError::PermissionDenied("logical context requires an agent".into())
            })?;
            runtime.active_contexts.insert(agent, context);
            Ok(())
        })
        .await
    }
}

pub(super) fn apply_native_schema_hints(tools: &mut Vec<ToolDef>, hints: &Value, native: bool) {
    let depth = hints["depth"].as_u64().unwrap_or(0);
    let maximum = hints["max_depth"].as_u64().unwrap_or(u64::MAX);
    tools.retain(|tool| {
        !(native
            && matches!(
                tool.tool_id.as_str(),
                "task" | "get_task_output" | "wait_tasks" | "kill_task"
            )
            || (depth >= maximum || hints["allowed_types"].as_array().is_some_and(Vec::is_empty))
                && matches!(tool.tool_id.as_str(), "spawn_subagent" | "task"))
    });
    for tool in tools {
        if !matches!(tool.tool_id.as_str(), "spawn_subagent" | "task") {
            continue;
        }
        let Some(properties) = tool
            .parameters
            .get_mut("properties")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        if hints["subagent_type"].is_object() {
            properties.insert("subagent_type".into(), hints["subagent_type"].clone());
        }
        if hints["model_selectable"] == false {
            properties.remove("model");
        }
    }
}
