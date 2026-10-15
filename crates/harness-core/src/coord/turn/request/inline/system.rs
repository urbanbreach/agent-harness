//! System prompt target extraction and carrier application.
use super::{Attachments, Content};
use crate::attachment_transport::AttachmentMetadata;
use crate::config::ArchiveSystemPrompt;
use harness_providers::{CompletionMessage, MessageContentPart, MessageRole};
use regex::Regex;
use std::sync::LazyLock;

/// Replaces the whole prompt: the real instructions ride the first user message as images.
const FULL_STUB: &str = "Your original operating instructions are authoritative, and they were \
moved into the images attached at the start of the first user message. Before you do anything \
else, read every one of those images in order, from the first to the last, and follow what they \
say as the instructions of this session.";

/// Replaces the loaded context-file instructions inside an otherwise intact prompt.
const CONTEXT_STUB: &str = "The instructions of the loaded context files were moved into the \
images attached at the start of the first user message. Read those images in order at this \
marker and apply what they say as if the original text stood here.";

/// Introduces the frames of a fully imaged prompt on the carrier.
const FULL_NOTE: &str = "Images with your operating instructions follow, in reading order.";

/// Introduces the frames of imaged context-file instructions on the carrier.
const CONTEXT_NOTE: &str = "Images with the content of the context files follow, in reading order.";

static REPO_RULES: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?s)<repo-rules>\n.*?\n</repo-rules>").ok());
static CONTEXT_FILES: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?s)## Context\n<instructions>\n.*?\n</instructions>").ok());

/// The text to image and what the leading system messages become without it.
#[derive(Debug)]
pub(super) struct Target {
    pub text: String,
    pub note: &'static str,
    pub replacement: Vec<CompletionMessage>,
}

fn system_message(text: String) -> CompletionMessage {
    CompletionMessage::text(MessageRole::System, text)
}

/// The target of `mode` among the leading system messages; none for an absent or empty
/// prompt, `none`, or an `agents_md` prompt without the exact regions.
pub(super) fn extract(mode: ArchiveSystemPrompt, leading: &[CompletionMessage]) -> Option<Target> {
    match mode {
        ArchiveSystemPrompt::None => None,
        ArchiveSystemPrompt::All => {
            let text = leading
                .iter()
                .map(|message| message.content.as_str())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (!text.trim().is_empty()).then(|| Target {
                text,
                note: FULL_NOTE,
                replacement: vec![system_message(FULL_STUB.to_owned())],
            })
        }
        ArchiveSystemPrompt::AgentsMd => regions(leading),
    }
}

/// The repo-rules and context-instructions regions, pattern order per block, trimmed and
/// joined with two LF; the first region of the prompt becomes the stub, the rest vanish.
fn regions(leading: &[CompletionMessage]) -> Option<Target> {
    let patterns = [REPO_RULES.as_ref()?, CONTEXT_FILES.as_ref()?];
    let mut extracted: Vec<&str> = Vec::new();
    let mut stub_placed = false;
    let mut replacement = Vec::with_capacity(leading.len());
    for message in leading {
        let block = message.content.as_str();
        let mut spans = Vec::new();
        for pattern in patterns {
            for found in pattern.find_iter(block) {
                extracted.push(found.as_str().trim());
                spans.push((found.start(), found.end()));
            }
        }
        if spans.is_empty() {
            replacement.push(message.clone());
            continue;
        }
        spans.sort_unstable();
        let (mut text, mut cursor) = (String::with_capacity(block.len()), 0);
        for (start, end) in spans {
            if start < cursor {
                continue;
            }
            text.push_str(&block[cursor..start]);
            if !stub_placed {
                text.push_str(CONTEXT_STUB);
                stub_placed = true;
            }
            cursor = end;
        }
        text.push_str(&block[cursor..]);
        if !text.trim().is_empty() {
            replacement.push(system_message(text));
        }
    }
    let text = extracted.join("\n\n");
    (!text.trim().is_empty()).then_some(Target {
        text,
        note: CONTEXT_NOTE,
        replacement,
    })
}

/// Puts the note and frames before the existing content of the first user message.
pub(super) fn carry(
    messages: &mut [CompletionMessage],
    carrier: usize,
    note: &str,
    frames: &[AttachmentMetadata],
    attachments: &mut Attachments,
    content: &mut Content,
) {
    let existing = std::mem::take(&mut messages[carrier].content);
    let originals = attachments.remove(&carrier).unwrap_or_default();
    let shift = frames.len();
    let mut parts = vec![MessageContentPart::Text(note.to_owned())];
    parts.extend((0..shift).map(MessageContentPart::Attachment));
    let mut list = frames.to_vec();
    if let Some(ordered) = content.remove(&carrier) {
        parts.extend(ordered.into_iter().map(|part| match part {
            MessageContentPart::Attachment(slot) => MessageContentPart::Attachment(slot + shift),
            text @ MessageContentPart::Text(_) => text,
        }));
        list.extend(originals);
    } else {
        if !existing.is_empty() {
            parts.push(MessageContentPart::Text(existing.clone()));
        }
        for original in originals {
            parts.push(MessageContentPart::Attachment(list.len()));
            list.push(original);
        }
    }
    messages[carrier].content = if existing.is_empty() {
        note.to_owned()
    } else {
        format!("{note}\n\n{existing}")
    };
    content.insert(carrier, parts);
    attachments.insert(carrier, list);
}

/// Swaps the `old` leading system messages for `replacement`, moving the attachment and
/// ordered content indices of everything after them.
pub(super) fn replace_leading(
    messages: &mut Vec<CompletionMessage>,
    old: usize,
    replacement: Vec<CompletionMessage>,
    attachments: &mut Attachments,
    content: &mut Content,
) {
    let new = replacement.len();
    messages.splice(..old, replacement);
    if new == old {
        return;
    }
    let moved = |index: usize| (index >= old).then(|| index - old + new);
    *attachments = std::mem::take(attachments)
        .into_iter()
        .filter_map(|(index, list)| Some((moved(index)?, list)))
        .collect();
    *content = std::mem::take(content)
        .into_iter()
        .filter_map(|(index, parts)| Some((moved(index)?, parts)))
        .collect();
}
