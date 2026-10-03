use harness_providers::{CompletionMessage, MessageRole};

#[derive(Clone, Default)]
pub(super) struct Context {
    pub entries: Vec<Entry>,
    pub usage: Vec<crate::subagent::FinalizedProviderUsage>,
    pub native_context_usage: Option<crate::subagent::SubagentContextUsage>,
    pub unavailable: Option<crate::subagent::FinalizedStateUnavailable>,
    pub model_request: Option<Box<harness_providers::CompletionRequest>>,
}
#[derive(Clone)]
pub(super) struct Entry {
    pub message: CompletionMessage,
    pub seq: u64,
    pub turn: Option<String>,
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
    pub settled_reasoning: Vec<String>,
    pub raw_tool_result: Option<crate::subagent::FinalizedToolResult>,
}
impl Context {
    pub fn new(system: &str) -> Self {
        let mut context = Self::default();
        if !system.is_empty() {
            context.push(
                CompletionMessage::text(MessageRole::System, system),
                0,
                None,
            );
        }
        context
    }
    pub fn push(&mut self, message: CompletionMessage, seq: u64, turn: Option<&str>) {
        self.entries.push(Entry {
            message,
            seq,
            turn: turn.map(str::to_owned),
            attachments: Vec::new(),
            settled_reasoning: Vec::new(),
            raw_tool_result: None,
        });
    }
    pub fn messages(&self) -> Vec<CompletionMessage> {
        self.entries.iter().map(|e| e.message.clone()).collect()
    }
    pub fn attachments(
        &self,
    ) -> std::collections::BTreeMap<usize, Vec<crate::attachment_transport::AttachmentMetadata>>
    {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.attachments.is_empty())
            .map(|(index, entry)| (index, entry.attachments.clone()))
            .collect()
    }
    pub fn discard_turn(&mut self, turn: &str) {
        self.entries.retain(|e| e.turn.as_deref() != Some(turn));
    }
    pub fn tokens(&self) -> u32 {
        tokens(&self.entries)
    }
}
pub(super) fn tokens(entries: &[Entry]) -> u32 {
    // ponytail: byte estimates choose retention; dispatch still uses the provider's full request budget.
    entries.iter().fold(0u32, |total, entry| {
        let bytes = entry
            .message
            .assistant_tool_calls
            .iter()
            .flatten()
            .fold(entry.message.content.len(), |bytes, call| {
                bytes.saturating_add(call.arguments_json.len())
            });
        let bytes = entry.attachments.iter().fold(bytes, |n, a| {
            n.saturating_add(usize::try_from(a.size).unwrap_or(usize::MAX))
        });
        total.saturating_add(
            u32::try_from(bytes.div_ceil(4))
                .unwrap_or(u32::MAX)
                .saturating_add(8),
        )
    })
}

/// The native child counter excludes request framing and tool schemas.
/// Unrepresentable attachment projections remain unknown.
pub(super) fn native_tokens(entries: &[Entry]) -> Option<u64> {
    entries.iter().try_fold(0u64, |total, entry| {
        if entry
            .attachments
            .iter()
            .any(|a| !a.mime.starts_with("image/"))
        {
            return None;
        }
        let bytes = entry
            .message
            .assistant_tool_calls
            .iter()
            .flatten()
            .fold(entry.message.content.len(), |bytes, call| {
                bytes.saturating_add(call.arguments_json.len())
            });
        let reasoning = entry
            .settled_reasoning
            .iter()
            .fold(0u64, |n, text| n.saturating_add(text.len() as u64 / 4));
        Some(
            total
                .saturating_add(bytes as u64 / 4)
                .saturating_add(reasoning)
                .saturating_add((entry.attachments.len() as u64).saturating_mul(765)),
        )
    })
}
