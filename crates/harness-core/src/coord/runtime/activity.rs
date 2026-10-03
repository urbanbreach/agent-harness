use super::*;

impl Agent {
    pub(in crate::coord) fn observe_activity(&mut self, event: &EventEnvelopeV1) {
        if matches!(&event.payload, EventV1::TaskScheduled(task)
            if task.state == TaskScheduleState::Started
                && self.attempt.as_deref() == Some(task.task_id.as_str()))
        {
            self.prompt_turns = self.prompt_turns.saturating_add(1);
            self.pending_model_tools.clear();
        }
        match &event.payload {
            EventV1::AssistantMessageFinished(message) => {
                self.pending_model_tools = message
                    .parts
                    .iter()
                    .filter_map(|part| {
                        if let crate::session::AssistantPart::ToolCall(call) = part {
                            Some((call.tool_call_id.to_string(), (call.tool_id.clone(), false)))
                        } else {
                            None
                        }
                    })
                    .collect();
            }
            EventV1::ToolCallStarted(tool) => {
                if let Some((_, started)) =
                    self.pending_model_tools.get_mut(tool.tool_call_id.as_str())
                {
                    *started = true;
                }
            }
            EventV1::ToolCallFinished(tool) => {
                if let Some((name, true)) =
                    self.pending_model_tools.remove(tool.tool_call_id.as_str())
                {
                    self.tool_calls = self.tool_calls.saturating_add(1);
                    if !self.tools_used.contains(&name) {
                        self.tools_used.push(name);
                    }
                }
            }
            EventV1::ProviderRequestFinished(response) if response.finish_reason == "error" => {
                self.error_count = self.error_count.saturating_add(1);
            }
            _ => {}
        }
    }
}
