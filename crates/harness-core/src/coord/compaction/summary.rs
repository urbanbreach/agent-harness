//! Summary generation prompts and helper functions.
//!
//! Ports Pi's `buildSummarizationPrompt`, `buildTurnPrefixSummaryPrompt`,
//! `formatFileOperations`, and `serializeConversation` into Rust, operating on
//! [`ConversationMessage`] instead of Pi's `AgentMessage`/`Message` types.

use crate::conversation::{
    ConversationAssistantMessage, ConversationMessage, ConversationToolResultMessage,
};
use crate::ids::RequestId;

use super::file_ops::{compute_file_lists, FileOperations};

// ---------------------------------------------------------------------------
// Prompt constants — copied verbatim from Pi
// ---------------------------------------------------------------------------

/// System prompt for the summarization LLM call.
///
/// Copied from Pi's `utils.ts` `SUMMARIZATION_SYSTEM_PROMPT`.
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

/// Initial summarization prompt (no previous summary).
///
/// Copied from Pi's `compaction.ts` `SUMMARIZATION_PROMPT`.
pub const SUMMARIZATION_PROMPT: &str = "The messages above are a conversation to summarize. Create a structured context checkpoint summary that another LLM will use to continue the work.\n\nUse this EXACT format:\n\n## Goal\n[What is the user trying to accomplish? Can be multiple items if the session covers different tasks.]\n\n## Constraints & Preferences\n- [Any constraints, preferences, or requirements mentioned by user]\n- [Or \"(none)\" if none were mentioned]\n\n## Progress\n### Done\n- [x] [Completed tasks/changes]\n\n### In Progress\n- [ ] [Current work]\n\n### Blocked\n- [Issues preventing progress, if any]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale]\n\n## Next Steps\n1. [Ordered list of what should happen next]\n\n## Critical Context\n- [Any data, examples, or references needed to continue]\n- [Or \"(none)\" if not applicable]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

/// Update summarization prompt (when a previous summary exists).
///
/// Copied from Pi's `compaction.ts` `UPDATE_SUMMARIZATION_PROMPT`.
pub const UPDATE_SUMMARIZATION_PROMPT: &str = "The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.\n\nUpdate the existing structured summary with new information. RULES:\n- PRESERVE all existing information from the previous summary\n- ADD new progress, decisions, and context from the new messages\n- UPDATE the Progress section: move items from \"In Progress\" to \"Done\" when completed\n- UPDATE \"Next Steps\" based on what was accomplished\n- PRESERVE exact file paths, function names, and error messages\n- If something is no longer relevant, you may remove it\n\nUse this EXACT format:\n\n## Goal\n[Preserve existing goals, add new ones if the task expanded]\n\n## Constraints & Preferences\n- [Preserve existing, add new ones discovered]\n\n## Progress\n### Done\n- [x] [Include previously done items AND newly completed items]\n\n### In Progress\n- [ ] [Current work - update based on progress]\n\n### Blocked\n- [Current blockers - remove if resolved]\n\n## Key Decisions\n- **[Decision]**: [Brief rationale] (preserve all previous, add new)\n\n## Next Steps\n1. [Update based on current state]\n\n## Critical Context\n- [Preserve important context, add new if needed]\n\nKeep each section concise. Preserve exact file paths, function names, and error messages.";

/// Turn-prefix summarization prompt (for split turns).
///
/// Copied from Pi's `compaction.ts` `TURN_PREFIX_SUMMARIZATION_PROMPT`.
pub const TURN_PREFIX_SUMMARIZATION_PROMPT: &str = "This is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.\n\nSummarize the prefix to provide context for the retained suffix:\n\n## Original Request\n[What did the user ask for in this turn?]\n\n## Early Progress\n- [Key decisions and work done in the prefix]\n\n## Context for Suffix\n- [Information needed to understand the retained recent work]\n\nBe concise. Focus on what's needed to understand the kept suffix.";

// ---------------------------------------------------------------------------
// Serialization helpers
// ---------------------------------------------------------------------------

/// Maximum characters for a tool result in serialized summaries.
///
/// Mirrors Pi's `TOOL_RESULT_MAX_CHARS`.
const TOOL_RESULT_MAX_CHARS: usize = 2000;

/// Truncate text to a maximum character length for summarization.
///
/// Keeps the beginning and appends a truncation marker. Mirrors Pi's
/// `truncateForSummary`.
fn truncate_for_summary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let truncated_chars = text.len() - max_chars;
    format!(
        "{}\n\n[... {} more characters truncated]",
        &text[..max_chars],
        truncated_chars
    )
}

/// Serialize conversation messages to text for summarization.
///
/// Formats each message as `[User]: text`, `[Assistant]: text`,
/// `[Assistant tool calls]: ...`, or `[Tool result]: ...`, joined by `\n\n`.
///
/// Tool results are truncated to [`TOOL_RESULT_MAX_CHARS`] characters to keep
/// the summarization request within reasonable token budgets.
///
/// Ports Pi's `serializeConversation`, operating on [`ConversationMessage`]
/// instead of Pi's `Message[]`.
pub fn serialize_conversation(messages: &[ConversationMessage]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        match msg {
            ConversationMessage::User(user) => {
                if !user.text.is_empty() {
                    parts.push(format!("[User]: {}", user.text));
                }
            }
            ConversationMessage::Assistant(assistant) => {
                serialize_assistant_message(assistant, &mut parts);
            }
            ConversationMessage::ToolResult(tool_result) => {
                serialize_tool_result_message(tool_result, &mut parts);
            }
            ConversationMessage::Checkpoint(_) => {
                // Checkpoint messages are not part of the conversation to summarize.
            }
        }
    }

    parts.join("\n\n")
}

fn serialize_assistant_message(assistant: &ConversationAssistantMessage, parts: &mut Vec<String>) {
    if !assistant.text.is_empty() {
        parts.push(format!("[Assistant]: {}", assistant.text));
    }

    if !assistant.tool_calls.is_empty() {
        let tool_calls: Vec<String> = assistant
            .tool_calls
            .iter()
            .map(|tc| format!("{}({})", tc.tool_id, tc.args_summary))
            .collect();
        parts.push(format!("[Assistant tool calls]: {}", tool_calls.join("; ")));
    }
}

fn serialize_tool_result_message(
    tool_result: &ConversationToolResultMessage,
    parts: &mut Vec<String>,
) {
    if let Some(content) = tool_result.output_summary.as_deref() {
        if !content.is_empty() {
            let truncated = truncate_for_summary(content, TOOL_RESULT_MAX_CHARS);
            parts.push(format!("[Tool result]: {}", truncated));
        }
    }
}

// ---------------------------------------------------------------------------
// File operations formatting
// ---------------------------------------------------------------------------

/// Format file operations as XML tags for summary.
///
/// Wraps read files in `<read-files>` and modified files in `<modified-files>`
/// tags. Returns an empty string when both lists are empty.
///
/// Ports Pi's `formatFileOperations`.
pub fn format_file_operations(read_files: &[String], modified_files: &[String]) -> String {
    let mut sections: Vec<String> = Vec::new();
    if !read_files.is_empty() {
        sections.push(format!(
            "<read-files>\n{}\n</read-files>",
            read_files.join("\n")
        ));
    }
    if !modified_files.is_empty() {
        sections.push(format!(
            "<modified-files>\n{}\n</modified-files>",
            modified_files.join("\n")
        ));
    }
    if sections.is_empty() {
        return String::new();
    }
    format!("\n\n{}", sections.join("\n\n"))
}

// ---------------------------------------------------------------------------
// Prompt builders
// ---------------------------------------------------------------------------

/// Build the summarization prompt for the history portion of a compaction.
///
/// Constructs a prompt containing:
/// 1. The serialized conversation wrapped in `<conversation>` tags
/// 2. The previous summary wrapped in `<previous-summary>` tags (if present)
/// 3. The base prompt ([`SUMMARIZATION_PROMPT`] or [`UPDATE_SUMMARIZATION_PROMPT`])
/// 4. Custom instructions appended as `Additional focus: ...` (if present)
/// 5. File operations context (read/modified file lists)
///
/// Ports Pi's inline prompt building from `generateSummary`.
pub fn build_summarization_prompt(
    messages: &[ConversationMessage],
    previous_summary: Option<&str>,
    custom_instructions: Option<&str>,
    file_ops: &FileOperations,
) -> String {
    let conversation_text = serialize_conversation(messages);

    let mut prompt_text = format!("<conversation>\n{conversation_text}\n</conversation>\n\n");

    if let Some(prev) = previous_summary {
        prompt_text.push_str(&format!(
            "<previous-summary>\n{prev}\n</previous-summary>\n\n"
        ));
    }

    let base_prompt = if previous_summary.is_some() {
        UPDATE_SUMMARIZATION_PROMPT
    } else {
        SUMMARIZATION_PROMPT
    };

    prompt_text.push_str(base_prompt);

    if let Some(instructions) = custom_instructions {
        prompt_text.push_str(&format!("\n\nAdditional focus: {instructions}"));
    }

    let (read_files, modified_files) = compute_file_lists(file_ops);
    let file_ops_text = format_file_operations(&read_files, &modified_files);
    if !file_ops_text.is_empty() {
        prompt_text.push_str(&file_ops_text);
    }

    prompt_text
}

/// Build the turn-prefix summarization prompt for a split turn.
///
/// Constructs a prompt containing the serialized turn-prefix messages wrapped in
/// `<conversation>` tags, followed by [`TURN_PREFIX_SUMMARIZATION_PROMPT`].
///
/// When `is_split_turn` is true, the caller should generate both the history
/// summary (via [`build_summarization_prompt`]) and the turn-prefix summary
/// (via this function), then combine the LLM results with a `---` separator:
///
/// ```text
/// {history_summary}\n\n---\n\n**Turn Context (split turn):**\n\n{turn_prefix_summary}
/// ```
///
/// Ports Pi's inline prompt building from `generateTurnPrefixSummary`.
pub fn build_turn_prefix_prompt(messages: &[ConversationMessage]) -> String {
    let conversation_text = serialize_conversation(messages);
    format!(
        "<conversation>\n{conversation_text}\n</conversation>\n\n{TURN_PREFIX_SUMMARIZATION_PROMPT}"
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation::{
        ConversationAssistantMessage, ConversationToolCall, ConversationToolResultMessage,
        ConversationUserMessage,
    };
    use crate::event::ToolCallStatus;
    use crate::ids::{RequestId, ToolCallId};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn user_msg(text: &str) -> ConversationMessage {
        ConversationMessage::User(ConversationUserMessage {
            request_id: RequestId::new("req-1"),
            text: text.to_string(),
            seq: None,
            agent_id: None,
        })
    }

    fn assistant_msg(text: &str) -> ConversationMessage {
        ConversationMessage::Assistant(ConversationAssistantMessage {
            request_id: RequestId::new("req-1"),
            agent_id: None,
            text: text.to_string(),
            tool_calls: Vec::new(),
            stop_reason: None,
            first_seq: None,
            last_seq: None,
            provider_id: None,
            model_id: None,
            output_digest: None,
        })
    }

    fn assistant_with_tool_calls(text: &str, calls: &[(&str, &str)]) -> ConversationMessage {
        let tool_calls = calls
            .iter()
            .map(|(tool_id, args_summary)| ConversationToolCall {
                tool_call_id: ToolCallId::new("tc-1"),
                tool_id: tool_id.to_string(),
                args_summary: args_summary.to_string(),
                args_digest: String::new(),
                seq: None,
                metadata: None,
            })
            .collect();
        ConversationMessage::Assistant(ConversationAssistantMessage {
            request_id: RequestId::new("req-1"),
            agent_id: None,
            text: text.to_string(),
            tool_calls,
            stop_reason: None,
            first_seq: None,
            last_seq: None,
            provider_id: None,
            model_id: None,
            output_digest: None,
        })
    }

    fn tool_result_msg(summary: &str) -> ConversationMessage {
        ConversationMessage::ToolResult(Box::new(ConversationToolResultMessage {
            request_id: RequestId::new("req-1"),
            tool_call_id: ToolCallId::new("tc-1"),
            tool_id: Some("read".to_string()),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(summary.to_string()),
            output_digest: None,
            output_json: None,
            seq: None,
            metadata: None,
        }))
    }

    // -----------------------------------------------------------------------
    // Prompt constants — required sections
    // -----------------------------------------------------------------------

    #[test]
    fn summarization_prompt_contains_required_sections() {
        for heading in [
            "## Goal",
            "## Constraints & Preferences",
            "## Progress",
            "### Done",
            "### In Progress",
            "### Blocked",
            "## Key Decisions",
            "## Next Steps",
            "## Critical Context",
        ] {
            assert!(
                SUMMARIZATION_PROMPT.contains(heading),
                "SUMMARIZATION_PROMPT missing required section: {heading}"
            );
        }
    }

    #[test]
    fn update_summarization_prompt_contains_required_sections() {
        for heading in [
            "## Goal",
            "## Constraints & Preferences",
            "## Progress",
            "### Done",
            "### In Progress",
            "### Blocked",
            "## Key Decisions",
            "## Next Steps",
            "## Critical Context",
        ] {
            assert!(
                UPDATE_SUMMARIZATION_PROMPT.contains(heading),
                "UPDATE_SUMMARIZATION_PROMPT missing required section: {heading}"
            );
        }
    }

    #[test]
    fn turn_prefix_prompt_contains_required_sections() {
        for heading in [
            "## Original Request",
            "## Early Progress",
            "## Context for Suffix",
        ] {
            assert!(
                TURN_PREFIX_SUMMARIZATION_PROMPT.contains(heading),
                "TURN_PREFIX_SUMMARIZATION_PROMPT missing required section: {heading}"
            );
        }
    }

    #[test]
    fn system_prompt_instructs_not_to_continue_conversation() {
        assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("Do NOT continue the conversation"));
        assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("ONLY output the structured summary"));
    }

    // -----------------------------------------------------------------------
    // format_file_operations
    // -----------------------------------------------------------------------

    #[test]
    fn format_file_operations_empty_returns_empty_string() {
        assert_eq!(format_file_operations(&[], &[]), "");
    }

    #[test]
    fn format_file_operations_read_only() {
        let read = vec!["src/lib.rs".to_string(), "README.md".to_string()];
        let result = format_file_operations(&read, &[]);
        assert!(result.starts_with("\n\n"));
        assert!(result.contains("<read-files>"));
        assert!(result.contains("src/lib.rs"));
        assert!(result.contains("README.md"));
        assert!(result.contains("</read-files>"));
        assert!(!result.contains("<modified-files>"));
    }

    #[test]
    fn format_file_operations_modified_only() {
        let modified = vec!["Cargo.toml".to_string()];
        let result = format_file_operations(&[], &modified);
        assert!(result.starts_with("\n\n"));
        assert!(result.contains("<modified-files>"));
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("</modified-files>"));
        assert!(!result.contains("<read-files>"));
    }

    #[test]
    fn format_file_operations_both() {
        let read = vec!["src/lib.rs".to_string()];
        let modified = vec!["Cargo.toml".to_string()];
        let result = format_file_operations(&read, &modified);
        assert!(result.contains("<read-files>"));
        assert!(result.contains("src/lib.rs"));
        assert!(result.contains("</read-files>"));
        assert!(result.contains("<modified-files>"));
        assert!(result.contains("Cargo.toml"));
        assert!(result.contains("</modified-files>"));
    }

    // -----------------------------------------------------------------------
    // serialize_conversation
    // -----------------------------------------------------------------------

    #[test]
    fn serialize_conversation_user_message() {
        let messages = vec![user_msg("Hello, world")];
        let result = serialize_conversation(&messages);
        assert_eq!(result, "[User]: Hello, world");
    }

    #[test]
    fn serialize_conversation_assistant_message() {
        let messages = vec![assistant_msg("I can help with that.")];
        let result = serialize_conversation(&messages);
        assert_eq!(result, "[Assistant]: I can help with that.");
    }

    #[test]
    fn serialize_conversation_tool_result() {
        let messages = vec![tool_result_msg("File contents here")];
        let result = serialize_conversation(&messages);
        assert_eq!(result, "[Tool result]: File contents here");
    }

    #[test]
    fn serialize_conversation_assistant_with_tool_calls() {
        let messages = vec![assistant_with_tool_calls(
            "Let me read that file.",
            &[("read", "path=src/main.rs")],
        )];
        let result = serialize_conversation(&messages);
        assert!(result.contains("[Assistant]: Let me read that file."));
        assert!(result.contains("[Assistant tool calls]: read(path=src/main.rs)"));
    }

    #[test]
    fn serialize_conversation_multiple_tool_calls() {
        let messages = vec![assistant_with_tool_calls(
            "",
            &[("read", "path=a.rs"), ("edit", "path=b.rs")],
        )];
        let result = serialize_conversation(&messages);
        assert!(result.contains("[Assistant tool calls]: read(path=a.rs); edit(path=b.rs)"));
    }

    #[test]
    fn serialize_conversation_full_dialogue() {
        let messages = vec![
            user_msg("Read README.md"),
            assistant_with_tool_calls("", &[("read", "path=README.md")]),
            tool_result_msg("# Project\n\nA test project."),
            assistant_msg("The project is a test project."),
        ];
        let result = serialize_conversation(&messages);
        assert!(result.contains("[User]: Read README.md"));
        assert!(result.contains("[Assistant tool calls]: read(path=README.md)"));
        assert!(result.contains("[Tool result]: # Project"));
        assert!(result.contains("[Assistant]: The project is a test project."));
        // Parts are separated by double newlines
        assert!(result.contains("\n\n[Assistant tool calls]:"));
        assert!(result.contains("\n\n[Tool result]:"));
        assert!(result.contains("\n\n[Assistant]:"));
    }

    #[test]
    fn serialize_conversation_truncates_long_tool_results() {
        let long_text = "x".repeat(TOOL_RESULT_MAX_CHARS + 500);
        let messages = vec![tool_result_msg(&long_text)];
        let result = serialize_conversation(&messages);
        assert!(result.contains("[Tool result]:"));
        assert!(result.contains("[... 500 more characters truncated]"));
    }

    #[test]
    fn serialize_conversation_skips_empty_user_text() {
        let messages = vec![user_msg("")];
        let result = serialize_conversation(&messages);
        assert_eq!(result, "");
    }

    #[test]
    fn serialize_conversation_skips_empty_tool_result() {
        let messages = vec![ConversationMessage::ToolResult(Box::new(
            ConversationToolResultMessage {
                request_id: RequestId::new("req-1"),
                tool_call_id: ToolCallId::new("tc-1"),
                tool_id: None,
                status: ToolCallStatus::Succeeded,
                output_summary: None,
                output_digest: None,
                output_json: None,
                seq: None,
                metadata: None,
            },
        ))];
        let result = serialize_conversation(&messages);
        assert_eq!(result, "");
    }

    // -----------------------------------------------------------------------
    // build_summarization_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn build_summarization_prompt_no_previous_summary() {
        let messages = vec![user_msg("Build a feature"), assistant_msg("Working on it.")];
        let file_ops = FileOperations::new();
        let prompt = build_summarization_prompt(&messages, None, None, &file_ops);

        assert!(prompt.starts_with("<conversation>"));
        assert!(prompt.contains("[User]: Build a feature"));
        assert!(prompt.contains("[Assistant]: Working on it."));
        assert!(prompt.contains("</conversation>"));
        assert!(prompt.contains(SUMMARIZATION_PROMPT));
        assert!(!prompt.contains("<previous-summary>"));
        assert!(!prompt.contains(UPDATE_SUMMARIZATION_PROMPT));
    }

    #[test]
    fn build_summarization_prompt_with_previous_summary() {
        let messages = vec![user_msg("Continue the work")];
        let file_ops = FileOperations::new();
        let prompt =
            build_summarization_prompt(&messages, Some("## Goal\nPrevious goal"), None, &file_ops);

        assert!(prompt.contains("<previous-summary>"));
        assert!(prompt.contains("## Goal\nPrevious goal"));
        assert!(prompt.contains("</previous-summary>"));
        assert!(prompt.contains(UPDATE_SUMMARIZATION_PROMPT));
        assert!(!prompt.contains(SUMMARIZATION_PROMPT));
    }

    #[test]
    fn build_summarization_prompt_with_custom_instructions() {
        let messages = vec![user_msg("Do something")];
        let file_ops = FileOperations::new();
        let prompt = build_summarization_prompt(
            &messages,
            None,
            Some("Focus on security implications"),
            &file_ops,
        );

        assert!(prompt.contains("Additional focus: Focus on security implications"));
    }

    #[test]
    fn build_summarization_prompt_includes_file_operations() {
        let messages = vec![user_msg("Read and edit files")];
        let mut file_ops = FileOperations::new();
        file_ops.read.insert("src/lib.rs".to_string());
        file_ops.edited.insert("Cargo.toml".to_string());

        let prompt = build_summarization_prompt(&messages, None, None, &file_ops);

        assert!(prompt.contains("<read-files>"));
        assert!(prompt.contains("src/lib.rs"));
        assert!(prompt.contains("<modified-files>"));
        assert!(prompt.contains("Cargo.toml"));
    }

    #[test]
    fn build_summarization_prompt_no_file_ops_omits_tags() {
        let messages = vec![user_msg("Hello")];
        let file_ops = FileOperations::new();
        let prompt = build_summarization_prompt(&messages, None, None, &file_ops);

        assert!(!prompt.contains("<read-files>"));
        assert!(!prompt.contains("<modified-files>"));
    }

    // -----------------------------------------------------------------------
    // build_turn_prefix_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn build_turn_prefix_prompt_structure() {
        let messages = vec![
            user_msg("Fix the bug in auth.rs"),
            assistant_msg("I'll start by reading the file."),
        ];
        let prompt = build_turn_prefix_prompt(&messages);

        assert!(prompt.starts_with("<conversation>"));
        assert!(prompt.contains("[User]: Fix the bug in auth.rs"));
        assert!(prompt.contains("[Assistant]: I'll start by reading the file."));
        assert!(prompt.contains("</conversation>"));
        assert!(prompt.contains(TURN_PREFIX_SUMMARIZATION_PROMPT));
    }

    // -----------------------------------------------------------------------
    // Split turn combination
    // -----------------------------------------------------------------------

    #[test]
    fn split_turn_prompts_combined_with_separator() {
        // When is_split_turn is true, the caller builds both prompts and
        // combines the LLM results with a --- separator.
        let history_messages = vec![user_msg("Build feature X"), assistant_msg("Done.")];
        let turn_prefix_messages =
            vec![user_msg("Also fix bug Y"), assistant_msg("Working on it.")];

        let history_prompt =
            build_summarization_prompt(&history_messages, None, None, &FileOperations::new());
        let turn_prefix_prompt = build_turn_prefix_prompt(&turn_prefix_messages);

        // Both prompts are independently valid
        assert!(history_prompt.contains(SUMMARIZATION_PROMPT));
        assert!(turn_prefix_prompt.contains(TURN_PREFIX_SUMMARIZATION_PROMPT));

        // The LLM results would be combined as:
        // {history_summary}\n\n---\n\n**Turn Context (split turn):**\n\n{turn_prefix_summary}
        let combined = format!(
            "{}\n\n---\n\n**Turn Context (split turn):**\n\n{}",
            "history summary", "turn prefix summary"
        );
        assert!(combined.contains("---"));
        assert!(combined.contains("**Turn Context (split turn):**"));
    }

    // -----------------------------------------------------------------------
    // truncate_for_summary
    // -----------------------------------------------------------------------

    #[test]
    fn truncate_for_summary_short_text_unchanged() {
        let text = "short text";
        assert_eq!(truncate_for_summary(text, 100), text);
    }

    #[test]
    fn truncate_for_summary_long_text_truncated() {
        let text = "x".repeat(150);
        let result = truncate_for_summary(&text, 100);
        assert!(result.starts_with(&"x".repeat(100)));
        assert!(result.contains("[... 50 more characters truncated]"));
    }
}
