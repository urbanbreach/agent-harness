//! Tool result candidates and the mixed text, frame and original image swap.
use super::cache::{fingerprint, frames_of, tool_label, Renderer};
use super::planner::{saves, ToolCandidate, FLOOR_TOKENS};
use super::{Attachments, Content, Env, Fact};
use crate::attachment_transport::AttachmentMetadata;
use crate::coord::context::archive::inline_cache::{InlineCache, ToolSlot};
use harness_providers::{CompletionMessage, MessageContentPart, MessageRole};
use std::{collections::HashSet, sync::Arc};

/// Tells the reader why a tool result arrives as pictures.
const NOTE: &str = "The output of this tool call was delivered as PNG images on purpose, to save \
context. The images hold the output verbatim, except that each position where the original \
result carried an image is marked with a numbered placeholder. Any original images follow the \
output images. Do not run the call again and do not report a tool malfunction because of how \
the output was delivered.";

fn source_marker(number: usize) -> String {
    format!("[Source image {number} was attached here in the original tool result.]")
}

fn original_label(number: usize) -> String {
    format!("[Original source image {number}; corresponds to its marker in the compacted text.]")
}

pub(super) fn is_image(attachment: &AttachmentMetadata) -> bool {
    attachment.mime.starts_with("image/")
}

/// A tool result that will be sent as frames when the plan keeps it.
pub(super) struct Built {
    pub index: usize,
    pub id: String,
    pub tokens: u32,
    pub frames: Arc<[AttachmentMetadata]>,
}

pub(super) struct Candidates {
    pub built: Vec<Built>,
    /// Every tool call id of the request, for cache eviction.
    pub live: HashSet<String>,
}

impl Candidates {
    pub(super) fn planner(&self) -> Vec<ToolCandidate> {
        self.built
            .iter()
            .map(|tool| ToolCandidate {
                index: tool.index,
                frames: tool.frames.len(),
            })
            .collect()
    }
}

/// Name of the tool a result answers: taken from the assistant call with the same id.
fn tool_name<'a>(messages: &'a [CompletionMessage], index: usize, id: &str) -> Option<&'a str> {
    messages[..index]
        .iter()
        .rev()
        .filter(|message| message.role == MessageRole::Assistant)
        .flat_map(|message| message.assistant_tool_calls.iter().flatten())
        .find(|call| call.tool_call_id == id)
        .map(|call| call.function_name.as_str())
        .or(messages[index].name.as_deref())
}

/// Text of a tool result as the reader would image it: text blocks joined with LF, each
/// source image position replaced by its numbered marker.
fn candidate_text(content: &str, images: usize) -> String {
    let mut parts = Vec::with_capacity(images + 1);
    if !content.is_empty() {
        parts.push(content.to_owned());
    }
    parts.extend((1..=images).map(source_marker));
    parts.join("\n")
}

/// Every tool result that passes the gates: not an error, `read` only for an opted-in
/// reader, at least 3000 text tokens, frames, and the 0.9 savings margin at the raw price.
pub(super) fn candidates(
    env: &Env,
    messages: &[CompletionMessage],
    attachments: &Attachments,
    facts: &[Fact],
    cache: &mut InlineCache,
    renderer: &Renderer,
) -> Candidates {
    let mut result = Candidates {
        built: Vec::new(),
        live: HashSet::new(),
    };
    let domain = env.counter.domain_id();
    let raw = env.shape.raw_frame_tokens();
    for (index, message) in messages.iter().enumerate() {
        let Some(id) = message
            .tool_call_id
            .as_deref()
            .filter(|_| message.role == MessageRole::Tool)
        else {
            continue;
        };
        result.live.insert(id.to_owned());
        if facts.get(index).is_some_and(|fact| fact.is_error)
            || (!env.read_opt_in && tool_name(messages, index, id) == Some("read"))
        {
            continue;
        }
        let images = attachments
            .get(&index)
            .map_or(0, |list| list.iter().filter(|a| is_image(a)).count());
        let key = fingerprint(&[message.content.as_bytes(), &(images as u64).to_le_bytes()]);
        let text = || candidate_text(&message.content, images);
        if cache
            .tools
            .get(id)
            .is_none_or(|slot| slot.text != key || slot.domain != domain)
        {
            cache.tools.insert(
                id.to_owned(),
                ToolSlot {
                    text: key,
                    domain,
                    tokens: env.counter.count_text(&text()),
                    render: None,
                },
            );
        }
        let Some(slot) = cache.tools.get_mut(id) else {
            continue;
        };
        let tokens = slot.tokens;
        if tokens < FLOOR_TOKENS || !saves(tokens, 1, raw) {
            continue;
        }
        let rendered = renderer.render(&mut slot.render, key, text, env.limit, &tool_label(id));
        let Some(frames) = frames_of(&rendered) else {
            continue;
        };
        if saves(tokens, frames.len(), raw) {
            result.built.push(Built {
                index,
                id: id.to_owned(),
                tokens,
                frames: Arc::clone(frames),
            });
        }
    }
    result
}

/// Swaps one tool result in the outgoing copy: note text, the archive frames, then every
/// original image preceded by its correspondence label. Other attachments follow.
pub(super) fn apply(
    message: &mut CompletionMessage,
    index: usize,
    frames: &[AttachmentMetadata],
    attachments: &mut Attachments,
    content: &mut Content,
) {
    let originals = attachments.remove(&index).unwrap_or_default();
    let (images, others): (Vec<_>, Vec<_>) = originals.into_iter().partition(is_image);
    let mut parts = vec![MessageContentPart::Text(NOTE.to_owned())];
    let mut text = vec![NOTE.to_owned()];
    let mut list = Vec::with_capacity(frames.len() + images.len() + others.len());
    for frame in frames {
        parts.push(MessageContentPart::Attachment(list.len()));
        list.push(frame.clone());
    }
    for (number, image) in images.into_iter().enumerate() {
        let label = original_label(number + 1);
        parts.push(MessageContentPart::Text(label.clone()));
        text.push(label);
        parts.push(MessageContentPart::Attachment(list.len()));
        list.push(image);
    }
    for other in others {
        parts.push(MessageContentPart::Attachment(list.len()));
        list.push(other);
    }
    message.content = text.join("\n");
    content.insert(index, parts);
    attachments.insert(index, list);
}
