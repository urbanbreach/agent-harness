//! Stop handling: what keeps a turn going when the model answers without a tool call.
use super::*;

/// Tools whose results show the agent still sees its todo list.
const TODO_TOOLS: [&str; 2] = ["todoread", "todowrite"];

impl Worker {
    /// The model answered without tool calls. Returns true when the turn should continue.
    /// `maintenance_owns_next_call` is set when the post-turn pass scheduled a continuation
    /// or paused automatic work on purpose; queued input still wins, but no reminder
    /// restarts the loop.
    pub(in crate::coord) async fn continue_after_stop(
        &mut self,
        text: &str,
        messages: &mut Context,
        maintenance_owns_next_call: bool,
    ) -> Result<bool, CoordinatorError> {
        // Inputs arriving during the final provider request belong to this turn,
        // including one-shot CLI turns which stop the runtime after completion.
        if self.take_turn_inputs(messages, true).await? {
            return Ok(true);
        }
        if maintenance_owns_next_call || self.is_parked() {
            return Ok(false);
        }
        if let Some(body) = self.output_contract_reminder(text).await? {
            self.remind(RuntimeReminderKind::OutputContract, body, None, messages)
                .await?;
            return Ok(true);
        }
        let settings = &self.behavior.todo_continuation;
        if !settings.enabled
            || self.guidance.todo_reminders >= settings.max_reminders
            || (self.guidance.todo_reminders > 0 && !self.guidance.acted_since_todo_reminder)
        {
            return Ok(false);
        }
        let Some(body) = self.open_todo_body().await? else {
            return Ok(false);
        };
        self.guidance.todo_reminders += 1;
        self.guidance.acted_since_todo_reminder = false;
        self.remind(RuntimeReminderKind::TodoContinuation, body, None, messages)
            .await?;
        Ok(true)
    }

    /// After a compaction resume the summary may have dropped the todo list. Restates the
    /// open items once, unless a todo result is still in context. It never forces a tool.
    pub(in crate::coord) async fn reinject_todos(
        &mut self,
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        let settings = &self.behavior.todo_continuation;
        if !settings.enabled
            || self.guidance.todo_reminders >= settings.max_reminders
            || surviving_todo_result(messages)
        {
            return Ok(());
        }
        let Some(body) = self.open_todo_body().await? else {
            return Ok(());
        };
        self.guidance.todo_reminders += 1;
        self.guidance.acted_since_todo_reminder = false;
        self.remind(RuntimeReminderKind::TodoContinuation, body, None, messages)
            .await
    }

    /// The reminder text for the root agent's open items; `None` when none are open.
    async fn open_todo_body(&self) -> Result<Option<String>, CoordinatorError> {
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        let open: Vec<_> = self
            .handle
            .root_todo_items(agent)
            .await?
            .into_iter()
            .filter(crate::proj::TodoItem::is_open)
            .collect();
        Ok((!open.is_empty()).then(|| todo_body(&open)))
    }
}

fn todo_body(open: &[crate::proj::TodoItem]) -> String {
    let items = open
        .iter()
        .map(|item| format!("- [{}] {}", item.status, item.content))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Your todo list still has open items:\n{items}\n\nContinue with the next open item now. When an item is done, mark it completed in the todo list; if it is blocked or no longer needed, mark it cancelled and say why in your final message. End the turn only when every item is completed or cancelled, or when you need input that only the user can give."
    )
}

/// A todo tool result is still among the kept entries.
fn surviving_todo_result(messages: &Context) -> bool {
    messages.entries.iter().any(|entry| {
        entry.message.role == MessageRole::Tool
            && entry
                .message
                .name
                .as_deref()
                .is_some_and(|name| TODO_TOOLS.contains(&name))
    })
}

impl CoordinatorHandle {
    /// Todo items for a root agent; subagents never inherit the session list.
    async fn root_todo_items(
        &self,
        agent: String,
    ) -> Result<Vec<crate::proj::TodoItem>, CoordinatorError> {
        self.call(move |s| Ok(s.root_todo_items(&agent))).await
    }
}

impl crate::coord::runtime::Runtime {
    pub(in crate::coord) fn root_todo_items(&self, agent: &str) -> Vec<crate::proj::TodoItem> {
        if self
            .agents
            .get(agent)
            .is_some_and(|agent| agent.info.parent_agent_id.is_none())
        {
            self.todos.items()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(role: MessageRole, name: Option<&str>) -> crate::coord::context::Entry {
        let message = CompletionMessage {
            continuation: None,
            role,
            content: String::new(),
            name: name.map(str::to_owned),
            tool_call_id: None,
            assistant_tool_calls: None,
        };
        crate::coord::context::Entry::new(message, 1, None)
    }

    #[test]
    fn todo_results_survive_only_as_tool_entries() {
        for (name, entries, expected) in [
            ("empty", vec![], false),
            (
                "write result",
                vec![entry(MessageRole::Tool, Some("todowrite"))],
                true,
            ),
            (
                "other tool",
                vec![entry(MessageRole::Tool, Some("read"))],
                false,
            ),
            (
                "same name on a user entry",
                vec![entry(MessageRole::User, Some("todoread"))],
                false,
            ),
        ] {
            let context = Context {
                entries,
                ..Default::default()
            };
            assert_eq!(surviving_todo_result(&context), expected, "{name}");
        }
    }
}
