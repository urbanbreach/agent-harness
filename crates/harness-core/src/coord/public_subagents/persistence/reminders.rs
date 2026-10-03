use super::*;

impl Runtime {
    pub(in crate::coord) fn native_completion_wake_text(
        &mut self,
        parent: &str,
        completion: &str,
        request: &str,
        fallback: String,
    ) -> Result<Option<String>, CoordinatorError> {
        let child = self.native_subagents.iter().find(|(_, child)| {
            child.registration.spawner == parent && child.request.as_deref() == Some(completion)
        });
        let Some((id, child)) = child else {
            return Ok(Some(fallback));
        };
        if child.consumed {
            return Ok(None);
        }
        let id = id.clone();
        let buffered = self.buffered_native_completions(parent);
        if buffered.contains(&id) {
            let text = self.native_completion_digest(parent, &buffered);
            for id in buffered {
                self.record_native_completion_delivery(&id, parent, request)?;
            }
            Ok(Some(text))
        } else {
            self.record_native_completion_delivery(&id, parent, request)?;
            Ok(Some(fallback))
        }
    }

    fn record_native_completion_delivery(
        &mut self,
        id: &str,
        parent: &str,
        request: &str,
    ) -> Result<u64, CoordinatorError> {
        let event = self.emit(
            EventActor::new(ActorKind::Worker, Some(parent.into())),
            Some(request.into()),
            EventV1::NativeSubagentReceipt(Box::new(NativeSubagentReceipt {
                payload_version: 1,
                child_id: id.into(),
                attempt_id: self.agents[id].attempt.clone(),
                generation: self.agents[id].generation,
                kind: "completion_reminder_delivered".into(),
                waiter_id: Some(request.into()),
            })),
        )?;
        if let Some(child) = self.native_subagents.get_mut(id) {
            child.buffered_for = None;
            child.consumed = true;
        }
        Ok(event.seq)
    }

    pub(in crate::coord) fn drain_native_completion_reminders(
        &mut self,
        parent: &str,
        request: &str,
    ) -> Result<Vec<(u64, String)>, CoordinatorError> {
        let buffered = self.buffered_native_completions(parent);
        if buffered.is_empty() {
            return Ok(Vec::new());
        }
        let text = self.native_completion_digest(parent, &buffered);
        let mut seq = 0;
        for id in buffered {
            seq = self.record_native_completion_delivery(&id, parent, request)?;
        }
        Ok(vec![(seq, text)])
    }

    pub(in crate::coord) fn attach_native_tool_reminders(
        &mut self,
        id: &str,
        job: &Job,
        result: &mut Result<ToolResult, CoordinatorError>,
        raw: &mut Option<Result<ToolResult, String>>,
    ) -> Result<(), CoordinatorError> {
        let (Some(parent), Some(request), Ok(output)) =
            (job.actor.agent_id.as_deref(), job.parent.as_deref(), result)
        else {
            return Ok(());
        };
        if !self
            .agents
            .get(parent)
            .is_some_and(|agent| agent.pending_model_tools.contains_key(id))
        {
            return Ok(());
        }
        let reminders = self.take_native_tool_reminders(parent, request)?;
        if !reminders.is_empty() {
            append(&mut output.display_text, &reminders);
            if let Some(Ok(raw)) = raw {
                append(&mut raw.display_text, &reminders);
            }
        }
        Ok(())
    }

    fn take_native_tool_reminders(
        &mut self,
        parent: &str,
        request: &str,
    ) -> Result<String, CoordinatorError> {
        let buffered = self.buffered_native_completions(parent);
        let mut reminders = Vec::new();
        for id in buffered {
            reminders.push(self.native_completion_reminder(&id, parent));
            self.record_native_completion_delivery(&id, parent, request)?;
        }
        Ok(reminders.join("\n\n"))
    }

    fn buffered_native_completions(&self, parent: &str) -> Vec<String> {
        let mut buffered: Vec<_> = self
            .native_subagents
            .iter()
            .filter(|(_, child)| child.buffered_for.as_deref() == Some(parent) && !child.consumed)
            .map(|(id, child)| (child.completion_age, id.clone()))
            .collect();
        buffered.sort();
        buffered.into_iter().map(|(_, id)| id).collect()
    }
}

fn append(output: &mut String, reminders: &str) {
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(reminders);
}
