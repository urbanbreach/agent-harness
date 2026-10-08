use super::*;

impl Runtime {
    pub(super) fn native_completion_reminder(&self, id: &str, parent: &str) -> String {
        let child = &self.native_subagents[id];
        let snapshot = child.updates.borrow();
        wrap(&format!(
            "Background subagent \"{id}\" ({}: \"{}\") {}.\n{}",
            child.registration.subagent_type,
            child.registration.description,
            outcome(&snapshot.result.status).0,
            self.native_reminder_output(id, parent),
        ))
    }

    pub(super) fn native_completion_digest(&self, parent: &str, ids: &[String]) -> String {
        let n = ids.len();
        let label = if n == 1 { "subagent" } else { "subagents" };
        let mut text = format!("While you were idle, {n} background {label} completed:\n");
        for (index, id) in ids.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            let child = &self.native_subagents[id];
            let snapshot = child.updates.borrow();
            let calls = self
                .subagent_history
                .records
                .get(id)
                .and_then(|record| record.accounting)
                .map_or(0, |a| a.tool_calls);
            text.push_str(&format!(
                "- [{}] {:?} — {} ({:.1}s, {calls} tool calls)\n{}\n",
                child.registration.subagent_type,
                child.registration.description,
                outcome(&snapshot.result.status).1,
                snapshot.result.duration_secs,
                self.native_reminder_output(id, parent),
            ));
        }
        wrap(&text)
    }

    fn native_reminder_output(&self, id: &str, parent: &str) -> String {
        let snapshot = self.native_subagents[id].updates.borrow();
        let mut result = snapshot.result.clone();
        let poll = self.agents.get(parent).and_then(|agent| {
            ["get_command_or_subagent_output", "get_task_output"]
                .into_iter()
                .find(|name| {
                    self.config.tool_registry.allows(&agent.profile, name)
                        && self
                            .config
                            .tool_registry
                            .get_for(name, self.tool_scope(Some(parent)).as_deref())
                            .is_some()
                })
        });
        // With a poll tool the parent can fetch everything, so the notice clips the child's
        // answer and leaves out a large structured result, keeping the metadata trailer.
        if let (Some(poll), Some(completed)) = (poll, &snapshot.completed) {
            let structured = completed
                .structured_output
                .as_ref()
                .map_or(0, |value| value.to_string().len());
            if completed.output.len() > REMINDER_OUTPUT_LIMIT
                || structured > REMINDER_STRUCTURED_LIMIT
            {
                let mut clipped = completed.clone();
                if completed.output.len() > REMINDER_OUTPUT_LIMIT {
                    let end = completed.output.floor_char_boundary(REMINDER_OUTPUT_LIMIT);
                    clipped.output.truncate(end);
                    clipped.output.push_str(&format!(
                        "\n[output truncated: {end} of {} bytes shown]",
                        completed.output.len(),
                    ));
                }
                if structured > REMINDER_STRUCTURED_LIMIT {
                    clipped.structured_output = None;
                    clipped.output.push_str(&format!(
                        "\n[structured_output left out of this notice: {structured} bytes]"
                    ));
                }
                clipped.output.push_str(&format!(
                    "\nUse {poll}(\"{id}\") to see the full output and structured_output."
                ));
                result.output = super::completed_body(&clipped);
            }
        }
        result.to_prompt_text()
    }
}

/// Completion notices carry at most this much of the child's answer.
const REMINDER_OUTPUT_LIMIT: usize = 16_000;
/// Larger structured results are left to explicit polling.
const REMINDER_STRUCTURED_LIMIT: usize = 8_000;

fn outcome(status: &str) -> (&'static str, &'static str) {
    match status {
        "completed" => ("completed successfully", "completed successfully"),
        "failed" => ("completed with failure", "failed"),
        "cancelled" => ("was cancelled", "cancelled"),
        _ => ("is still running", "running"),
    }
}

fn wrap(text: &str) -> String {
    let text =
        ["system-reminder", "system_reminder"]
            .into_iter()
            .fold(text.to_owned(), |text, tag| {
                text.replace(&format!("</{tag}"), &format!("<\\/{tag}"))
                    .replace(&format!("<{tag}"), &format!("<\\{tag}"))
            });
    format!("<system-reminder>\n{text}\n</system-reminder>")
}
