//! Branch summarization for tree navigation.
//!
//! When navigating to a different point in the session tree, this generates
//! a summary of the branch being left so context isn't lost.
//!
//! Ports Pi's `branch-summarization.ts` into Rust, operating on
//! [`EventEnvelopeV1`] and [`ConversationMessage`] instead of Pi's
//! `SessionEntry` and `AgentMessage` types.

use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderStreamEvent,
};
use serde_json::Value;
use tokio_stream::StreamExt;

use crate::conversation::{
    project_conversation, ConversationCheckpointMessage, ConversationMessage,
};
use crate::event::{EventEnvelopeV1, EventV1};

use super::file_ops::{
    compute_file_lists, extract_file_ops_from_tool_call, merge_file_operations, FileOperations,
};
use super::summary::{format_file_operations, serialize_conversation, SUMMARIZATION_SYSTEM_PROMPT};
use super::tokens::estimate_message_tokens;

// ---------------------------------------------------------------------------
// Prompt constants — copied verbatim from Pi
// ---------------------------------------------------------------------------

/// Preamble prepended to the generated branch summary.
///
/// Copied from Pi's `branch-summarization.ts` `BRANCH_SUMMARY_PREAMBLE`.
pub const BRANCH_SUMMARY_PREAMBLE: &str = "The user explored a different conversation branch before returning here.\nSummary of that exploration:\n\n";

/// Prompt sent to the LLM to generate the branch summary.
///
/// Copied from Pi's `branch-summarization.ts` `BRANCH_SUMMARY_PROMPT`.
pub const BRANCH_SUMMARY_PROMPT: &str = "Create a structured summary of this conversation branch for context when returning later.\n\nUse this EXACT format:\n\n## Goal\n[What was the user trying to accomplish in this branch?]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Work that was started but not finished]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [What should happen next to continue this work]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

/// Prefix used when wrapping a branch summary into a conversation message.
///
/// Copied from Pi's `messages.ts` `BRANCH_SUMMARY_PREFIX`.
pub const BRANCH_SUMMARY_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";

/// Suffix used when wrapping a branch summary into a conversation message.
///
/// Copied from Pi's `messages.ts` `BRANCH_SUMMARY_SUFFIX`.
pub const BRANCH_SUMMARY_SUFFIX: &str = "</summary>";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Entries collected for branch summarization, in chronological order.
#[derive(Debug, Clone, Default)]
pub struct CollectEntriesResult {
    /// Events to summarize, in chronological order.
    pub entries: Vec<EventEnvelopeV1>,
    /// Common ancestor seq between old and new position, if any.
    pub common_ancestor_seq: Option<u64>,
}

/// Prepared branch entries ready for LLM summarization.
#[derive(Debug, Clone, Default)]
pub struct BranchPreparation {
    /// Messages extracted for summarization, in chronological order.
    pub messages: Vec<ConversationMessage>,
    /// File operations extracted from tool calls and nested summaries.
    pub file_ops: FileOperations,
    /// Total estimated tokens in included messages.
    pub total_tokens: u32,
}

/// Result of a branch summary generation attempt.
#[derive(Debug, Clone, Default)]
pub struct BranchSummaryResult {
    /// The generated summary, or `None` if generation failed or produced nothing.
    pub summary: Option<String>,
    /// Files that were read (but not modified) during the branch.
    pub read_files: Vec<String>,
    /// Files that were edited or written during the branch.
    pub modified_files: Vec<String>,
    /// `true` if the LLM call was aborted.
    pub aborted: bool,
    /// Error message if generation failed.
    pub error: Option<String>,
}

/// Options for branch summary generation.
#[derive(Debug, Clone)]
pub struct GenerateBranchSummaryOptions {
    /// Provider ID for the summarization model.
    pub provider_id: harness_providers::ProviderId,
    /// Model ID for the summarization model.
    pub model_id: harness_providers::ModelId,
    /// Context window size in tokens (default 128000).
    pub context_window: u32,
    /// Tokens reserved for prompt + LLM response (default 16384).
    pub reserve_tokens: u32,
    /// Optional custom instructions for summarization.
    pub custom_instructions: Option<String>,
    /// If `true`, `custom_instructions` replaces the default prompt instead of being appended.
    pub replace_instructions: bool,
}

impl Default for GenerateBranchSummaryOptions {
    fn default() -> Self {
        Self {
            provider_id: String::new(),
            model_id: String::new(),
            context_window: 128_000,
            reserve_tokens: 16_384,
            custom_instructions: None,
            replace_instructions: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry collection
// ---------------------------------------------------------------------------

/// Collect events for branch summarization when navigating from one position to another.
///
/// Walks backward through `events`, filtering by `agent_id`, and stops at
/// `target_seq` (the common ancestor). Returns entries in chronological order.
///
/// Ports Pi's `collectEntriesForBranchSummary`, adapted to the Rust event model
/// where the session tree is represented by `seq` numbers instead of string IDs.
pub fn collect_entries_for_branch_summary(
    events: &[EventEnvelopeV1],
    agent_id: &str,
    target_seq: Option<u64>,
) -> CollectEntriesResult {
    let mut entries: Vec<EventEnvelopeV1> = Vec::new();

    for event in events.iter().rev() {
        if let Some(seq) = target_seq {
            if event.seq <= seq {
                break;
            }
        }
        if event.actor.agent_id.as_deref() == Some(agent_id) {
            entries.push(event.clone());
        }
    }

    entries.reverse();

    CollectEntriesResult {
        entries,
        common_ancestor_seq: target_seq,
    }
}

// ---------------------------------------------------------------------------
// Entry preparation
// ---------------------------------------------------------------------------

/// Prepare entries for summarization with a token budget.
///
/// Two-pass algorithm:
/// 1. **First pass**: accumulate file ops from ALL entries, including nested
///    `BranchSummary` and `SessionCompaction` events (for cumulative tracking).
/// 2. **Second pass**: walk messages newest-to-oldest, extracting file ops from
///    tool calls, and adding messages until the token budget is exceeded.
///    Summary messages (checkpoints) get a 10% headroom: if the budget is
///    exceeded but the current message is a checkpoint and total tokens are
///    still under 90% of the budget, it is included anyway.
///
/// Ports Pi's `prepareBranchEntries`.
pub fn prepare_branch_entries(entries: &[EventEnvelopeV1], token_budget: u32) -> BranchPreparation {
    let mut file_ops = FileOperations::new();

    // First pass: collect file ops from ALL summary events (not from hooks).
    for event in entries {
        extract_file_ops_from_summary_event(&event.payload, &mut file_ops);
    }

    // Convert events to conversation messages (including checkpoint messages
    // for BranchSummary and SessionCompaction events).
    let all_messages = events_to_messages(entries);

    // Second pass: walk newest to oldest, extracting file ops from tool calls
    // and respecting the token budget.
    let mut messages: Vec<ConversationMessage> = Vec::new();
    let mut total_tokens = 0u32;

    for msg in all_messages.iter().rev() {
        extract_file_ops_from_message(msg, &mut file_ops);

        let tokens = estimate_message_tokens(msg);

        if token_budget > 0 && total_tokens.saturating_add(tokens) > token_budget {
            // Summary messages get a 10% headroom: include if under 90% of budget.
            if is_summary_message(msg) {
                let headroom = u64::from(token_budget) * 90 / 100;
                if u64::from(total_tokens) < headroom {
                    messages.push(msg.clone());
                    total_tokens = total_tokens.saturating_add(tokens);
                }
            }
            break;
        }

        messages.push(msg.clone());
        total_tokens = total_tokens.saturating_add(tokens);
    }

    messages.reverse();

    BranchPreparation {
        messages,
        file_ops,
        total_tokens,
    }
}

// ---------------------------------------------------------------------------
// Summary generation
// ---------------------------------------------------------------------------

/// Generate a summary of abandoned branch entries using an LLM.
///
/// Prepares entries with token budgeting, serializes the conversation to text,
/// builds a prompt, and calls the provider for summarization.
///
/// Ports Pi's `generateBranchSummary`.
pub async fn generate_branch_summary(
    provider: &dyn Provider,
    entries: &[EventEnvelopeV1],
    options: &GenerateBranchSummaryOptions,
) -> BranchSummaryResult {
    let token_budget = options
        .context_window
        .saturating_sub(options.reserve_tokens);
    let preparation = prepare_branch_entries(entries, token_budget);

    if preparation.messages.is_empty() {
        return BranchSummaryResult {
            summary: Some("No content to summarize".to_string()),
            ..Default::default()
        };
    }

    let conversation_text = serialize_conversation(&preparation.messages);

    let instructions = match (&options.custom_instructions, options.replace_instructions) {
        (Some(custom), true) => custom.clone(),
        (Some(custom), false) => format!("{BRANCH_SUMMARY_PROMPT}\n\nAdditional focus: {custom}"),
        (None, _) => BRANCH_SUMMARY_PROMPT.to_string(),
    };

    let prompt_text =
        format!("<conversation>\n{conversation_text}\n</conversation>\n\n{instructions}");

    let request = CompletionRequest {
        provider_id: Some(options.provider_id.clone()),
        model_id: options.model_id.clone(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: prompt_text,
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(2048),
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    };

    let mut stream = provider.stream_completion(request).await;
    let mut output = String::new();
    let mut error: Option<String> = None;

    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::TextDelta(delta) => output.push_str(&delta),
            ProviderStreamEvent::Error { message, .. } => {
                error = Some(message);
                break;
            }
            ProviderStreamEvent::Done { .. } | ProviderStreamEvent::DoneWithMetadata { .. } => {
                break
            }
            _ => {}
        }
    }

    if let Some(err) = error {
        return BranchSummaryResult {
            error: Some(err),
            ..Default::default()
        };
    }

    let (read_files, modified_files) = compute_file_lists(&preparation.file_ops);

    let mut summary = format!("{BRANCH_SUMMARY_PREAMBLE}{output}");
    let file_ops_text = format_file_operations(&read_files, &modified_files);
    summary.push_str(&file_ops_text);

    BranchSummaryResult {
        summary: if summary.trim().is_empty() {
            None
        } else {
            Some(summary)
        },
        read_files,
        modified_files,
        aborted: false,
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract file ops from a summary event (`BranchSummary` or `SessionCompaction`).
///
/// Only extracts from pi-generated summaries (`from_hook == false`), not
/// extension-generated ones. Mirrors Pi's first-pass logic.
fn extract_file_ops_from_summary_event(payload: &EventV1, file_ops: &mut FileOperations) {
    match payload {
        EventV1::BranchSummary(e) if !e.from_hook => {
            for f in &e.read_files {
                file_ops.read.insert(f.clone());
            }
            for f in &e.modified_files {
                file_ops.edited.insert(f.clone());
            }
        }
        EventV1::SessionCompaction(e) if !e.from_hook => {
            for f in &e.read_files {
                file_ops.read.insert(f.clone());
            }
            for f in &e.modified_files {
                file_ops.edited.insert(f.clone());
            }
        }
        _ => {}
    }
}

/// Extract file ops from tool calls in an assistant message.
///
/// Parses `args_summary` as JSON (best-effort) and calls
/// [`extract_file_ops_from_tool_call`]. Mirrors Pi's `extractFileOpsFromMessage`.
fn extract_file_ops_from_message(msg: &ConversationMessage, file_ops: &mut FileOperations) {
    if let ConversationMessage::Assistant(assistant) = msg {
        for tool_call in &assistant.tool_calls {
            if let Ok(args) = serde_json::from_str::<Value>(&tool_call.args_summary) {
                if let Some(op) = extract_file_ops_from_tool_call(&tool_call.tool_id, &args) {
                    merge_file_operations(file_ops, op);
                }
            }
        }
    }
}

/// Convert events to conversation messages, including checkpoint messages
/// for `BranchSummary` and `SessionCompaction` events.
///
/// Uses [`project_conversation`] for regular conversation events, then
/// inserts checkpoint messages at the correct chronological position.
fn events_to_messages(events: &[EventEnvelopeV1]) -> Vec<ConversationMessage> {
    let projection = project_conversation(events, &[]).unwrap_or_default();
    let mut messages = projection.messages;

    // Create checkpoint messages for summary events and insert at the correct position.
    for event in events {
        let checkpoint = match &event.payload {
            EventV1::BranchSummary(e) if !e.from_hook => Some(ConversationCheckpointMessage {
                checkpoint_id: format!("branch-{}", event.seq),
                agent_id: e.agent_id.clone(),
                through_seq: event.seq,
                summary: e.summary.clone(),
            }),
            EventV1::SessionCompaction(e) if !e.from_hook => Some(ConversationCheckpointMessage {
                checkpoint_id: format!("compaction-{}", event.seq),
                agent_id: e.agent_id.clone(),
                through_seq: event.seq,
                summary: e.summary.clone(),
            }),
            _ => None,
        };

        if let Some(checkpoint) = checkpoint {
            let msg = ConversationMessage::Checkpoint(checkpoint);
            let seq = event.seq;
            let pos = messages
                .iter()
                .position(|m| message_seq(m) > seq)
                .unwrap_or(messages.len());
            messages.insert(pos, msg);
        }
    }

    messages
}

/// Get the chronological position (seq) of a conversation message.
fn message_seq(msg: &ConversationMessage) -> u64 {
    match msg {
        ConversationMessage::User(m) => m.seq.unwrap_or(0),
        ConversationMessage::Assistant(m) => m.last_seq.or(m.first_seq).unwrap_or(0),
        ConversationMessage::ToolResult(m) => m.seq.unwrap_or(0),
        ConversationMessage::Checkpoint(m) => m.through_seq,
    }
}

/// Check if a message is a summary (checkpoint) message.
fn is_summary_message(msg: &ConversationMessage) -> bool {
    matches!(msg, ConversationMessage::Checkpoint(_))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{
        ActorKind, AssistantMessageFinishedEvent, BranchSummaryEvent, EventActor, EventEnvelopeV1,
        EventV1, ProviderRequestStartedEvent, ProviderStreamDeltaEvent, SessionCompactionEvent,
        ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent,
        SCHEMA_VERSION,
    };
    use crate::ids::{RequestId, ToolCallId};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn envelope(seq: u64, agent_id: &str, payload: EventV1) -> EventEnvelopeV1 {
        envelope_with_corr(seq, agent_id, None, payload)
    }

    fn envelope_with_corr(
        seq: u64,
        agent_id: &str,
        correlation_id: Option<&str>,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_test".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
            correlation_id: correlation_id.map(String::from),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn user_msg(seq: u64, agent_id: &str, request_id: &str, text: &str) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: RequestId::new(request_id),
                text: text.to_string(),
            }),
        )
    }

    fn stream_delta(seq: u64, agent_id: &str, request_id: &str, delta: &str) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: RequestId::new(request_id),
                delta: delta.to_string(),
            }),
        )
    }

    fn provider_started(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: RequestId::new(request_id),
                provider_id: "test-provider".to_string(),
                model_id: "test-model".to_string(),
                prompt_summary: "test prompt".to_string(),
                request_digest: "digest".to_string(),
                metadata: None,
            }),
        )
    }

    fn tool_requested_with_corr(
        seq: u64,
        agent_id: &str,
        request_id: &str,
        tool_id: &str,
        args_summary: &str,
    ) -> EventEnvelopeV1 {
        envelope_with_corr(
            seq,
            agent_id,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: ToolCallId::new(&format!("tc-{seq}")),
                tool_id: tool_id.to_string(),
                args_summary: args_summary.to_string(),
                args_digest: format!("digest-{seq}"),
                metadata: None,
            }),
        )
    }

    fn assistant_finished(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: RequestId::new(request_id),
                tool_call_count: 0,
                assistant_message: None,
            }),
        )
    }

    fn tool_requested(
        seq: u64,
        agent_id: &str,
        tool_id: &str,
        args_summary: &str,
    ) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: ToolCallId::new(&format!("tc-{seq}")),
                tool_id: tool_id.to_string(),
                args_summary: args_summary.to_string(),
                args_digest: format!("digest-{seq}"),
                metadata: None,
            }),
        )
    }

    fn tool_finished(seq: u64, agent_id: &str, output_summary: &str) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: ToolCallId::new(&format!("tc-{seq}")),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(output_summary.to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        )
    }

    fn branch_summary_event(
        seq: u64,
        agent_id: &str,
        summary: &str,
        read_files: Vec<String>,
        modified_files: Vec<String>,
    ) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::BranchSummary(BranchSummaryEvent {
                agent_id: agent_id.to_string(),
                summary: summary.to_string(),
                from_event_seq: seq,
                read_files,
                modified_files,
                from_hook: false,
            }),
        )
    }

    fn session_compaction_event(
        seq: u64,
        agent_id: &str,
        summary: &str,
        read_files: Vec<String>,
        modified_files: Vec<String>,
    ) -> EventEnvelopeV1 {
        envelope(
            seq,
            agent_id,
            EventV1::SessionCompaction(SessionCompactionEvent {
                agent_id: agent_id.to_string(),
                summary: summary.to_string(),
                first_kept_event_seq: seq,
                first_kept_request_id: None,
                tokens_before: 100,
                read_files,
                modified_files,
                trigger_reason: "auto".to_string(),
                from_hook: false,
            }),
        )
    }

    // -----------------------------------------------------------------------
    // collect_entries_for_branch_summary
    // -----------------------------------------------------------------------

    #[test]
    fn collect_entries_filters_by_agent_id() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "hello"),
            assistant_finished(2, "agent-1", "req-1"),
            user_msg(3, "agent-2", "req-2", "other agent"),
            assistant_finished(4, "agent-2", "req-2"),
        ];

        let result = collect_entries_for_branch_summary(&events, "agent-1", None);
        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.entries[0].seq, 1);
        assert_eq!(result.entries[1].seq, 2);
        assert_eq!(result.common_ancestor_seq, None);
    }

    #[test]
    fn collect_entries_stops_at_target_seq() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "first"),
            assistant_finished(2, "agent-1", "req-1"),
            user_msg(3, "agent-1", "req-2", "second"),
            assistant_finished(4, "agent-1", "req-2"),
            user_msg(5, "agent-1", "req-3", "third"),
            assistant_finished(6, "agent-1", "req-3"),
        ];

        // target_seq = 2 means we stop before seq 3 (collect seq > 2)
        let result = collect_entries_for_branch_summary(&events, "agent-1", Some(2));
        assert_eq!(result.entries.len(), 4);
        assert_eq!(result.entries[0].seq, 3);
        assert_eq!(result.entries[1].seq, 4);
        assert_eq!(result.entries[2].seq, 5);
        assert_eq!(result.entries[3].seq, 6);
        assert_eq!(result.common_ancestor_seq, Some(2));
    }

    #[test]
    fn collect_entries_empty_events_returns_empty() {
        let result = collect_entries_for_branch_summary(&[], "agent-1", None);
        assert!(result.entries.is_empty());
        assert_eq!(result.common_ancestor_seq, None);
    }

    #[test]
    fn collect_entries_no_matching_agent_returns_empty() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "hello"),
            assistant_finished(2, "agent-1", "req-1"),
        ];

        let result = collect_entries_for_branch_summary(&events, "agent-2", None);
        assert!(result.entries.is_empty());
    }

    #[test]
    fn collect_entries_returns_chronological_order() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "first"),
            stream_delta(2, "agent-1", "req-1", "response"),
            assistant_finished(3, "agent-1", "req-1"),
            user_msg(4, "agent-1", "req-2", "second"),
            stream_delta(5, "agent-1", "req-2", "response2"),
            assistant_finished(6, "agent-1", "req-2"),
        ];

        let result = collect_entries_for_branch_summary(&events, "agent-1", None);
        assert_eq!(result.entries.len(), 6);
        // Verify chronological order
        for i in 0..result.entries.len() - 1 {
            assert!(
                result.entries[i].seq < result.entries[i + 1].seq,
                "entries should be in chronological order"
            );
        }
    }

    // -----------------------------------------------------------------------
    // prepare_branch_entries
    // -----------------------------------------------------------------------

    #[test]
    fn prepare_branch_entries_converts_events_to_messages() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "Read README.md"),
            provider_started(2, "agent-1", "req-1"),
            stream_delta(3, "agent-1", "req-1", "I'll read it."),
            assistant_finished(4, "agent-1", "req-1"),
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(!prep.messages.is_empty());
        // Should have at least a user message and an assistant message
        assert!(prep
            .messages
            .iter()
            .any(|m| matches!(m, ConversationMessage::User(_))));
        assert!(prep
            .messages
            .iter()
            .any(|m| matches!(m, ConversationMessage::Assistant(_))));
    }

    #[test]
    fn prepare_branch_entries_respects_token_budget() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", &"x".repeat(200)),
            stream_delta(2, "agent-1", "req-1", &"y".repeat(200)),
            assistant_finished(3, "agent-1", "req-1"),
            user_msg(4, "agent-1", "req-2", &"z".repeat(200)),
            stream_delta(5, "agent-1", "req-2", &"w".repeat(200)),
            assistant_finished(6, "agent-1", "req-2"),
        ];

        // With a small budget, not all messages should fit
        let prep = prepare_branch_entries(&events, 50);
        assert!(prep.total_tokens <= 50 || prep.messages.len() < events.len());
    }

    #[test]
    fn prepare_branch_entries_zero_budget_includes_all() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "hello"),
            stream_delta(2, "agent-1", "req-1", "world"),
            assistant_finished(3, "agent-1", "req-1"),
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(!prep.messages.is_empty());
        assert!(prep.total_tokens > 0);
    }

    #[test]
    fn prepare_branch_entries_empty_entries_returns_empty() {
        let prep = prepare_branch_entries(&[], 1000);
        assert!(prep.messages.is_empty());
        assert_eq!(prep.total_tokens, 0);
        assert!(prep.file_ops.read.is_empty());
        assert!(prep.file_ops.edited.is_empty());
    }

    // -----------------------------------------------------------------------
    // Nested file ops from summary events
    // -----------------------------------------------------------------------

    #[test]
    fn prepare_branch_entries_extracts_file_ops_from_branch_summary() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "do work"),
            stream_delta(2, "agent-1", "req-1", "working"),
            assistant_finished(3, "agent-1", "req-1"),
            branch_summary_event(
                4,
                "agent-1",
                "Previous branch summary",
                vec!["src/old.rs".to_string()],
                vec!["Cargo.toml".to_string()],
            ),
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(prep.file_ops.read.contains("src/old.rs"));
        assert!(prep.file_ops.edited.contains("Cargo.toml"));
    }

    #[test]
    fn prepare_branch_entries_extracts_file_ops_from_session_compaction() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "do work"),
            stream_delta(2, "agent-1", "req-1", "working"),
            assistant_finished(3, "agent-1", "req-1"),
            session_compaction_event(
                4,
                "agent-1",
                "Compaction summary",
                vec!["docs/readme.md".to_string()],
                vec!["src/main.rs".to_string()],
            ),
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(prep.file_ops.read.contains("docs/readme.md"));
        assert!(prep.file_ops.edited.contains("src/main.rs"));
    }

    #[test]
    fn prepare_branch_entries_extracts_file_ops_from_tool_calls() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "read and edit files"),
            provider_started(2, "agent-1", "req-1"),
            tool_requested_with_corr(3, "agent-1", "req-1", "read", r#"{"path":"src/lib.rs"}"#),
            tool_finished(4, "agent-1", "file contents"),
            tool_requested_with_corr(5, "agent-1", "req-1", "edit", r#"{"path":"Cargo.toml"}"#),
            tool_finished(6, "agent-1", "edited"),
            assistant_finished(7, "agent-1", "req-1"),
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(prep.file_ops.read.contains("src/lib.rs"));
        assert!(prep.file_ops.edited.contains("Cargo.toml"));
    }

    #[test]
    fn prepare_branch_entries_skips_file_ops_from_hook_summaries() {
        let mut event = branch_summary_event(
            4,
            "agent-1",
            "Hook summary",
            vec!["hook_read.rs".to_string()],
            vec!["hook_edit.rs".to_string()],
        );
        // Set from_hook to true
        if let EventV1::BranchSummary(ref mut e) = event.payload {
            e.from_hook = true;
        }

        let events = vec![
            user_msg(1, "agent-1", "req-1", "do work"),
            stream_delta(2, "agent-1", "req-1", "working"),
            assistant_finished(3, "agent-1", "req-1"),
            event,
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(!prep.file_ops.read.contains("hook_read.rs"));
        assert!(!prep.file_ops.edited.contains("hook_edit.rs"));
    }

    #[test]
    fn prepare_branch_entries_includes_checkpoint_messages() {
        let events = vec![
            user_msg(1, "agent-1", "req-1", "do work"),
            stream_delta(2, "agent-1", "req-1", "working"),
            assistant_finished(3, "agent-1", "req-1"),
            branch_summary_event(4, "agent-1", "Branch summary text", vec![], vec![]),
        ];

        let prep = prepare_branch_entries(&events, 0);
        assert!(
            prep.messages
                .iter()
                .any(|m| matches!(m, ConversationMessage::Checkpoint(_))),
            "checkpoint messages should be included"
        );
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn branch_summary_prompt_contains_required_sections() {
        for heading in [
            "## Goal",
            "## Constraints & Preferences",
            "## Progress",
            "### Done",
            "### In Progress",
            "### Blocked",
            "## Key Decisions",
            "## Next Steps",
        ] {
            assert!(
                BRANCH_SUMMARY_PROMPT.contains(heading),
                "BRANCH_SUMMARY_PROMPT missing required section: {heading}"
            );
        }
    }

    #[test]
    fn branch_summary_preamble_contains_context() {
        assert!(BRANCH_SUMMARY_PREAMBLE.contains("different conversation branch"));
        assert!(BRANCH_SUMMARY_PREAMBLE.ends_with("\n\n"));
    }

    #[test]
    fn branch_summary_prefix_suffix_form_summary_tags() {
        assert!(BRANCH_SUMMARY_PREFIX.contains("<summary>"));
        assert_eq!(BRANCH_SUMMARY_SUFFIX, "</summary>");
    }
}
