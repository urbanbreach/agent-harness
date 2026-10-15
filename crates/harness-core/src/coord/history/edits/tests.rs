use super::*;
use crate::{attachment_transport::AttachmentMetadata, event::ContextEdit};
use harness_compaction::ItemPart;

fn entry(role: MessageRole, text: &str) -> Entry {
    let mut entry = Entry::new(CompletionMessage::text(role, text), 7, None);
    if role == MessageRole::Tool {
        entry.message.tool_call_id = Some("call".into());
    }
    entry
}

fn attachment(mime: &str) -> AttachmentMetadata {
    AttachmentMetadata::new("a", mime, 1, None, "blob:sha256:a")
}

fn apply(entry: Entry, edit: ContextEdit) -> Entry {
    let mut entries = [entry];
    apply_edits(&mut entries, &[edit], &HashMap::new());
    let [entry] = entries;
    entry
}

fn tool_edit(text: &str, drop_attachments: bool) -> ContextEdit {
    ContextEdit::ToolResult {
        seq: Some(7),
        tool_call_id: "call".into(),
        text: text.into(),
        pruned_at_ms: 99,
        drop_attachments,
    }
}

#[test]
fn pruned_results_keep_their_own_time_and_follow_the_attachment_flag() {
    for (replacement, drop, text, attachments) in [
        (
            "[Output truncated - 9 tokens]",
            true,
            "[Output truncated - 9 tokens]",
            0,
        ),
        ("[shaken ~9 tokens]", false, "[shaken ~9 tokens]", 1),
        ("", false, "[Output truncated]", 1),
    ] {
        let mut original = entry(MessageRole::Tool, "old output");
        original.at_ms = Some(5);
        original.attachments = vec![attachment("image/png")];
        let edited = apply(original, tool_edit(replacement, drop));
        assert_eq!(edited.message.content, text);
        assert_eq!(
            edited.at_ms,
            Some(5),
            "pruning never rewrites the result time"
        );
        assert_eq!(edited.pruned_at_ms, Some(99));
        assert!(edited.tool.pruned);
        assert_eq!(edited.attachments.len(), attachments);
        assert_eq!(edited.version, 1);
    }
}

#[test]
fn text_ranges_follow_ordered_parts_and_tally_elided_output() {
    let mut original = entry(MessageRole::Assistant, "headMIDDLEtail");
    original.parts = vec![
        ItemPart::text("head"),
        ItemPart::reasoning("thinking"),
        ItemPart::text("MIDDLEtail"),
    ];
    let edit = ContextEdit::TextRange {
        seq: 7,
        start: 4,
        end: 10,
        text: "[x]".into(),
        elided_output_tokens: 6,
    };
    let edited = apply(original.clone(), edit);
    assert_eq!(edited.message.content, "head[x]tail");
    assert_eq!(edited.elided_output_tokens, 6);
    assert_eq!(
        edited.parts,
        [
            ItemPart::text("head"),
            ItemPart::reasoning("thinking"),
            ItemPart::text("[x]tail")
        ]
    );
    // Parts that no longer reassemble the content are dropped rather than left stale.
    original.parts = vec![ItemPart::text("other")];
    let edit = ContextEdit::TextRange {
        seq: 7,
        start: 0,
        end: 4,
        text: String::new(),
        elided_output_tokens: 0,
    };
    assert!(apply(original, edit).parts.is_empty());
}

#[test]
fn dropping_reasoning_keeps_text_calls_and_unrelated_continuations() {
    let mut original = entry(MessageRole::Assistant, "answer");
    original.settled_reasoning = vec!["why".into()];
    original.parts = vec![
        ItemPart::reasoning("why"),
        ItemPart::RedactedReasoning,
        ItemPart::text("answer"),
        ItemPart::Call(0),
    ];
    let edited = apply(
        original,
        ContextEdit::DropReasoning {
            seq: 7,
            elided_output_tokens: 3,
        },
    );
    assert!(edited.settled_reasoning.is_empty());
    assert_eq!(edited.parts, [ItemPart::text("answer"), ItemPart::Call(0)]);
    assert_eq!(edited.message.content, "answer");
    assert_eq!(edited.elided_output_tokens, 3);
}

#[test]
fn image_removal_strips_only_images_and_marks_empty_entries() {
    for (text, attachments, left, content) in [
        ("look", vec!["image/png"], 0, "look"),
        ("", vec!["image/png"], 0, "[image removed]"),
        ("", vec!["image/png", "application/pdf"], 1, ""),
    ] {
        let mut original = entry(MessageRole::User, text);
        original.attachments = attachments.into_iter().map(attachment).collect();
        let edited = apply(
            original,
            ContextEdit::DropAttachments {
                seq: 7,
                tool_call_id: None,
            },
        );
        assert_eq!(edited.attachments.len(), left);
        assert_eq!(edited.message.content, content);
    }
}
