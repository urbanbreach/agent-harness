use super::*;

impl Runtime {
    pub fn restore_children(&mut self, events: &[EventEnvelopeV1]) {
        self.children.clear();
        let native: std::collections::BTreeSet<_> = events
            .iter()
            .filter_map(|event| {
                if let EventV1::NativeSubagentRegistered(registration) = &event.payload {
                    Some(registration.child_id.as_str())
                } else {
                    None
                }
            })
            .collect();
        for event in crate::conversation_rewind::active_events(events).iter() {
            match &event.payload {
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id,
                    parent_agent_id: Some(parent),
                    ..
                }) => {
                    if native.contains(agent_id.as_str()) {
                        continue;
                    }
                    let parent_session = if self.children.contains_key(parent) {
                        parent.clone()
                    } else {
                        event.run_id.to_string()
                    };
                    self.children.insert(
                        agent_id.clone(),
                        Child {
                            parent_agent: parent.clone(),
                            parent_tool: String::new(),
                            parent_request: None,
                            parent_session,
                            description: String::new(),
                            request: String::new(),
                            background: false,
                            complete: true,
                        },
                    );
                }
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    output_json: Some(value),
                    tool_call_id,
                    ..
                }) => {
                    let Some(child) = value["session_id"]
                        .as_str()
                        .and_then(|id| self.children.get_mut(id))
                    else {
                        continue;
                    };
                    if child.parent_tool == tool_call_id.as_str()
                        && event.actor.agent_id.as_ref() == Some(&child.parent_agent)
                    {
                        child.description =
                            value["description"].as_str().unwrap_or_default().into();
                        child.background = value["run_in_background"].as_bool().unwrap_or(false);
                    }
                }
                EventV1::UiIntentReceived(intent)
                    if intent.intent == "background_foreground_child" =>
                {
                    let Some(handle) = intent
                        .params
                        .get("session_id")
                        .or_else(|| intent.params.get("request_id"))
                        .or_else(|| intent.params.get("handle_id"))
                    else {
                        continue;
                    };
                    for (_, child) in self
                        .children
                        .iter_mut()
                        .filter(|(id, child)| *id == handle || child.request == *handle)
                    {
                        child.background = true;
                    }
                }
                _ => {}
            }
            let Some(child) = event
                .actor
                .agent_id
                .as_ref()
                .and_then(|id| self.children.get_mut(id))
            else {
                continue;
            };
            match &event.payload {
                EventV1::UserMessageSubmitted(prompt) => {
                    child.request = prompt.request_id.to_string();
                    child.complete = false;
                }
                EventV1::TaskScheduled(task) if task.task_id.as_str() == child.request => {
                    let Some(lineage) = task.metadata.as_ref().and_then(|m| m.lineage.as_ref())
                    else {
                        continue;
                    };
                    child.parent_tool = lineage.parent_tool_call_id.clone().unwrap_or_default();
                    child.parent_request = lineage.parent_request_id.clone();
                    child.parent_session = lineage
                        .parent_session_id
                        .clone()
                        .unwrap_or_else(|| child.parent_session.clone());
                }
                EventV1::TaskCompleted(task) if task.task_id.as_str() == child.request => {
                    child.complete = true;
                }
                EventV1::TaskCancelled(task) if task.task_id.as_str() == child.request => {
                    child.complete = true;
                }
                _ => {}
            }
        }
    }
}
