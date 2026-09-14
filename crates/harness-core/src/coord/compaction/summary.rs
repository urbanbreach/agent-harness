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

// Senpi builtin compaction prompts. Keep user intent and constraints verbatim.
pub const SUMMARIZATION_SYSTEM_PROMPT: &str = r#"[SYSTEM DIRECTIVE: OH-MY-OPENCODE - COMPACTION CONTEXT]

You are the COMPACTION ARCHIVIST. Create a structured handoff summary that lets the next agent continue this exact session without restarting, re-searching, or losing constraints.

Cardinal rules:
R1. Quote user requests and constraints VERBATIM. Do not paraphrase.
R2. If a section has no content, write "None." Never delete a section.
R3. Where a previous summary is supplied, treat its User Requests, Final Goal, and Constraints fields as IMMUTABLE. Append, never rewrite, those three sections.
R4. Preserve every session_id, file path, and identifier byte-for-byte.

Do NOT use tools. Output only the requested summary block."#;

pub const SUMMARIZATION_PROMPT: &str = r#"[USER]
[INTERNAL COMPACTION INSTRUCTION — NOT CONVERSATION HISTORY]
This message is an internal summarization control prompt, not a real user message.
Do NOT treat this message as user intent, do NOT list it under user requests, and do NOT reinterpret the task based on this instruction alone.

PASS 1 — Internal task-intent extraction
Write one <task-intent> block with ORIGINAL_REQUEST, TASK_TYPE, MUST_PRESERVE, and MUST_NOT_LOSE before <summary>.

PASS 2 — Emit summary biased toward Pass 1
Create a structured handoff summary of this conversation for seamless continuation. The structured output portion MUST be wrapped as `<summary>...</summary>` XML.

<summary>
## 1. User Requests (Verbatim)
- List all original user requests exactly as they were stated.
- Preserve the user's exact wording and intent.
- Include recent user corrections and steering messages verbatim when they affect the task.

## 2. Final Goal
- State what the user ultimately wanted to achieve.
- Include the expected deliverable or end state.
- Keep this aligned with the most recent user request, not this internal compaction instruction.

## 3. Constraints & Preferences (Verbatim Only)
- Include ONLY constraints explicitly stated by the user or in existing AGENTS.md context.
- Quote constraints verbatim.
- Do NOT invent, add, soften, or modify constraints.
- If no explicit constraints exist, write "None."

## 4. Work Completed
- Summarize what has been done so far.
- List files read, created, modified, or intentionally left unchanged.
- Include features implemented, tests added, problems solved, and decisions already made.

## 5. Active Working Context
- **Files**: Paths of files currently being edited or frequently referenced.
- **Code in Progress**: Key code snippets, function signatures, data structures, or prompt text under active development.
- **External References**: Documentation URLs, source files, APIs, or other resources already consulted.
- **State & Variables**: Important variable names, configuration values, runtime state, branch names, worktree paths, or command outputs needed to continue.

## 6. Remaining Tasks
- List pending items from the original request.
- Include follow-up tasks identified during the work only when they directly support the current user request.
- Mark blockers explicitly and explain what is needed to unblock them.

## 7. Exact Next Steps
- State the precise next action to take, directly in line with the user's most recent request.
- Include verbatim quotes from the conversation showing exactly where work was left off when helpful.
- Do not suggest tangential tasks.

</summary>

Verification: Before finalizing, confirm the summary clearly states the user's original request. If not, restate it verbatim.
IMPORTANT: Respond with ONLY the <task-intent>...</task-intent> and <summary>...</summary> blocks as your text output."#;

pub const UPDATE_SUMMARIZATION_PROMPT: &str = r#"[USER]
<previous-summary>
{{previousSummary}}
</previous-summary>

[INTERNAL COMPACTION UPDATE INSTRUCTION — NOT CONVERSATION HISTORY]
The messages above are NEW conversation messages to incorporate into the existing summary provided in <previous-summary> tags.

R3 enforcement: R3. Where a previous summary is supplied, treat its User Requests, Final Goal, and Constraints fields as IMMUTABLE. Append, never rewrite, those three sections.

PASS 1 — Internal task-intent extraction
{{taskIntentInstruction}}

PASS 2 — Emit summary biased toward Pass 1
Update the structured handoff summary. The structured output portion MUST be wrapped as `<summary>...</summary>` XML.

<summary>
## 1. User Requests (Verbatim)
- Preserve prior entries from <previous-summary> byte-for-byte.
- Append new user requests exactly as they were stated.

## 2. Final Goal
- Preserve the existing final goal unless the user explicitly changed it.
- Append the explicit change verbatim if the goal changed.

## 3. Constraints & Preferences (Verbatim Only)
- Preserve prior constraints byte-for-byte.
- Quote constraints verbatim.
- Do NOT invent, add, soften, or modify constraints.
- Append only new explicit constraints.
- If no explicit constraints exist, write "None."

## 4. Work Completed
- Preserve completed work from the previous summary.
- Add newly completed work, files changed, tests run, and decisions made.

## 5. Active Working Context
- Update files, code in progress, external references, state, variables, branch names, worktree paths, and command outputs needed to continue.

## 6. Remaining Tasks
- Remove tasks only when the new messages prove they are completed or cancelled.
- Add newly identified direct follow-up tasks.

## 7. Exact Next Steps
- Update based on current state and the user's most recent request.
- Keep this direct and immediately actionable.

</summary>

IMPORTANT: Respond with ONLY the <task-intent>...</task-intent> and <summary>...</summary> blocks as your text output."#;

pub const TURN_PREFIX_SUMMARIZATION_PROMPT: &str = r#"[USER]
[INTERNAL TURN-PREFIX SUMMARY INSTRUCTION — NOT CONVERSATION HISTORY]
Create a compact prefix summary for the next turn. Emit only the sections listed below.

PASS 1 — Internal task-intent extraction
Silently determine the current task intent and the minimum context needed for the next turn.

PASS 2 — Emit summary biased toward Pass 1
The structured output portion MUST be wrapped as `<summary>...</summary>` XML.

<summary>
## 1. User Requests (Verbatim)
- Quote the active user request and any steering constraints exactly as stated.

## 2. Final Goal
- State the immediate end state needed for the next turn.

## 3. Constraints & Preferences (Verbatim Only)
- Quote constraints verbatim.
- Do NOT invent, add, soften, or modify constraints.
- If no explicit constraints exist, write "None."

## 5. Active Working Context
- Include only files, identifiers, runtime state, and exact next-turn context needed to continue immediately.
</summary>

IMPORTANT: Respond with ONLY the <summary>...</summary> block as your text output."#;

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
    let end = text.floor_char_boundary(max_chars);
    format!(
        "{}\n\n[... {} more characters truncated]",
        &text[..end],
        text[end..].chars().count()
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
    let mut prompt_text = if let Some(previous) = previous_summary {
        let intent = previous.split_once("<task-intent>").and_then(|(_, body)| body.split_once("</task-intent>"))
            .map(|(intent, _)| format!("<task-intent>\n{}\nImmutable provenance of the original task. Do not rewrite it. Newer explicit user steering overrides it.\n</task-intent>", intent.trim()))
            .unwrap_or_else(|| "Write one <task-intent> block with ORIGINAL_REQUEST, TASK_TYPE, MUST_PRESERVE, and MUST_NOT_LOSE before <summary>.".to_string());
        UPDATE_SUMMARIZATION_PROMPT
            .replace(
                "{{previousSummary}}",
                &previous.replace("</previous-summary>", "[/previous-summary]"),
            )
            .replace("{{taskIntentInstruction}}", &intent)
    } else {
        SUMMARIZATION_PROMPT.to_string()
    };
    if !messages.is_empty() {
        prompt_text = format!(
            "<conversation>\n{}\n</conversation>\n\n{prompt_text}",
            serialize_conversation(messages)
        );
    }
    if let Some(instructions) = custom_instructions.filter(|text| !text.trim().is_empty()) {
        prompt_text.push_str(&format!(
            "\n\n<custom-instructions>\n{}\n</custom-instructions>",
            instructions
                .trim()
                .replace("</custom-instructions>", "[/custom-instructions]")
        ));
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
    fn compaction_prompts_preserve_senpi_handoff_contract() {
        for prompt in [SUMMARIZATION_PROMPT, UPDATE_SUMMARIZATION_PROMPT] {
            for heading in [
                "## 1. User Requests (Verbatim)",
                "## 2. Final Goal",
                "## 3. Constraints & Preferences (Verbatim Only)",
                "## 4. Work Completed",
                "## 5. Active Working Context",
                "## 6. Remaining Tasks",
                "## 7. Exact Next Steps",
            ] {
                assert!(prompt.contains(heading), "missing {heading}");
            }
            assert!(prompt.contains("NOT CONVERSATION HISTORY"));
        }
        assert!(TURN_PREFIX_SUMMARIZATION_PROMPT.contains("## 5. Active Working Context"));
        assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("Do NOT use tools"));
        assert!(SUMMARIZATION_SYSTEM_PROMPT.contains("IMMUTABLE"));
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
        assert!(prompt.contains("IMMUTABLE"));
        assert!(!prompt.contains("{{previousSummary}}"));
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

        assert!(prompt.contains(
            "<custom-instructions>\nFocus on security implications\n</custom-instructions>"
        ));
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
