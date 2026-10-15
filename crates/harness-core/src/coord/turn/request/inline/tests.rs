//! Shared builders of the inline imaging scenarios.
use super::{Attachments, Content, Env, Fact};
use crate::attachment_transport::AttachmentMetadata;
use crate::config::ArchiveSystemPrompt;
use crate::coord::context::archive::inline_cache::{FrameSink, InlineCache};
use harness_compaction::archive::{self, ArchiveShape, RenderedFrame};
use harness_compaction::tokenizer::Counter;
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

pub(super) fn with_sink<S: FrameSink + 'static>(sink: Arc<S>) -> InlineCache {
    InlineCache {
        sink: Some(sink),
        ..InlineCache::default()
    }
}

mod gates;
mod history;
mod journal;
mod runtime;
mod system;
mod tool_swap;

/// Frames per text by the prefix of the text before its first colon; unknown prefixes
/// render one frame.
pub(super) struct FakeSink {
    pub generation: AtomicU64,
    pub frames: Vec<(&'static str, usize)>,
    pub texts: Mutex<Vec<String>>,
}

impl FakeSink {
    pub(super) fn new(frames: &[(&'static str, usize)]) -> Self {
        Self {
            generation: AtomicU64::new(0),
            frames: frames.to_vec(),
            texts: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn calls(&self) -> usize {
        self.texts.lock().map_or(0, |texts| texts.len())
    }
}

impl FrameSink for FakeSink {
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    fn rasterize(
        &self,
        text: &str,
        _: &ArchiveShape,
        _: Option<u32>,
    ) -> Option<Vec<RenderedFrame>> {
        if let Ok(mut texts) = self.texts.lock() {
            texts.push(text.to_owned());
        }
        let label = text.split(':').next().unwrap_or_default();
        let count = self
            .frames
            .iter()
            .find(|(prefix, _)| *prefix == label)
            .map_or(1, |(_, count)| *count);
        Some(
            (0..count)
                .map(|n| RenderedFrame {
                    png: vec![u8::try_from(n).unwrap_or(0), 1, 2],
                    width: 8,
                    height: 8,
                    chars: 1,
                })
                .collect(),
        )
    }
}

/// The no-id Anthropic shape: square raw price 3136.
pub(super) fn shape() -> ArchiveShape {
    *archive::resolve_shape("", "anthropic", None)
}

pub(super) fn counter() -> Counter {
    Counter::byte_estimating(None)
}

pub(super) fn env(
    counter: &Counter,
    limit: u32,
    mode: ArchiveSystemPrompt,
    tools: bool,
) -> Env<'_> {
    Env {
        shape: shape(),
        limit,
        mode,
        tool_results: tools,
        read_opt_in: false,
        counter,
    }
}

/// Text of the label and a given number of tokens under the byte estimating counter.
pub(super) fn text(label: &str, tokens: usize) -> String {
    let filler = (tokens * 4).saturating_sub(label.len() + 1);
    format!("{label}:{}", "a".repeat(filler))
}

pub(super) fn message(role: MessageRole, content: &str) -> CompletionMessage {
    CompletionMessage::text(role, content)
}

pub(super) fn call(id: &str, tool: &str) -> CompletionMessage {
    let mut message = message(MessageRole::Assistant, "");
    message.assistant_tool_calls = Some(vec![AssistantToolCall {
        tool_call_id: id.into(),
        function_name: tool.into(),
        arguments_json: "{}".into(),
    }]);
    message
}

pub(super) fn tool(id: &str, content: &str) -> CompletionMessage {
    let mut message = message(MessageRole::Tool, content);
    message.tool_call_id = Some(id.into());
    message
}

pub(super) fn image(id: &str) -> AttachmentMetadata {
    AttachmentMetadata::from_bytes(id, "image/png", None, &[9, 9, 9], None)
}

pub(super) fn attach(attachments: &mut Attachments, index: usize, ids: &[&str]) {
    attachments.insert(index, ids.iter().map(|id| image(id)).collect());
}

pub(super) fn facts(count: usize) -> Vec<Fact> {
    (0..count)
        .map(|index| Fact {
            seq: Some(index as u64 + 1),
            at_ms: Some(1_000 + index as u64 * 10),
            ..Fact::default()
        })
        .collect()
}

pub(super) fn empty_content() -> Content {
    Content::new()
}
