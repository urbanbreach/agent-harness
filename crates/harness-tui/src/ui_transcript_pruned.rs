use super::*;
pub(super) fn apply_user(app: &AppState, target: u64, body: &mut String) {
    for event in app.events() {
        let EventV1::ContextPruned(pruned) = &event.payload else {
            continue;
        };
        for edit in &pruned.edits {
            if let ContextEdit::TextRange {
                seq,
                start,
                end,
                text,
                ..
            } = edit
                && *seq == target
            {
                replace_text(body, *start, *end, text);
            }
        }
    }
}

fn replace_text(body: &mut String, start: u32, end: u32, text: &str) {
    let start = usize::try_from(start).unwrap_or(usize::MAX);
    let end = usize::try_from(end).unwrap_or(usize::MAX);
    if start <= end && body.is_char_boundary(start) && body.is_char_boundary(end) {
        body.replace_range(start..end, &crate::text::strip_ansi_escapes(text));
    }
}
use harness_core::event::{ContextEdit, EventV1};

pub(super) fn apply(app: &AppState, parts: &mut Vec<(u64, TranscriptAssistantPart)>) {
    for event in app.events() {
        let EventV1::ContextPruned(pruned) = &event.payload else {
            continue;
        };
        for edit in &pruned.edits {
            let tool_id = resolve_tool_id(app, edit);
            apply_edit(app, parts, edit, tool_id);
        }
    }
}

fn apply_edit(
    app: &AppState,
    parts: &mut Vec<(u64, TranscriptAssistantPart)>,
    edit: &ContextEdit,
    tool_id: Option<&str>,
) {
    if let ContextEdit::DropReasoning { seq, .. } = edit {
        let target = app.reasoning_source_seq(*seq);
        parts.retain(|(source, part)| {
            (*source != *seq && *source != target)
                || !matches!(part, TranscriptAssistantPart::Reasoning(_))
        });
        return;
    }
    for (seq, part) in parts {
        match (edit, part) {
            (ContextEdit::ToolResult { text, .. }, TranscriptAssistantPart::ToolCall(tool))
                if tool_id == Some(tool.tool_call_id.as_str()) =>
            {
                tool.detail_blocks = vec![TranscriptToolCallDetailBlock::Message {
                    text: crate::text::strip_ansi_escapes(text),
                    tone: TranscriptToolCallDetailTone::Secondary,
                }];
                tool.header.subtitle = None;
            }
            (
                ContextEdit::TextRange {
                    seq: target,
                    start,
                    end,
                    text,
                    ..
                },
                TranscriptAssistantPart::Body(
                    TranscriptBodyBlock::RichText(body)
                    | TranscriptBodyBlock::StreamingRichText(body),
                ),
            ) if seq == target => {
                replace_text(body, *start, *end, text);
            }
            _ => {}
        }
    }
}

fn resolve_tool_id<'a>(app: &'a AppState, edit: &'a ContextEdit) -> Option<&'a str> {
    let ContextEdit::ToolResult {
        seq, tool_call_id, ..
    } = edit
    else {
        return None;
    };
    let Some(seq) = seq else {
        return Some(tool_call_id);
    };
    let event = app.events().find(|event| event.seq == *seq)?;
    let EventV1::AssistantMessageFinished(message) = &event.payload else {
        return None;
    };
    message.parts.iter().find_map(|part| match part {
        harness_core::session::AssistantPart::ToolCall(call)
            if call
                .provider_tool_call_id
                .as_deref()
                .unwrap_or(call.tool_call_id.as_str())
                == tool_call_id =>
        {
            Some(call.tool_call_id.as_str())
        }
        _ => None,
    })
}
