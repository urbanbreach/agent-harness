//! Borrowed view of one provider-visible context entry.
//!
//! The coordinator owns the real entries; every algorithm in this crate reads them
//! through [`Item`] so it stays free of journal, clock and transport concerns.
use harness_providers::{CompletionMessage, CompletionUsage, MessageRole};
use std::borrow::Cow;

pub use harness_tokenizer::ImageDetail;

/// Who authored an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    /// Typed by the human.
    #[default]
    User,
    /// A parent or peer steering message.
    Agent,
    /// Authored by the harness itself.
    Host,
}

/// What a harness-authored note is for. Only meaningful on [`ItemKind::Note`] entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Reminder,
    CompactionResume,
    LengthStop,
    ChildCompletion,
    Wake,
    Peer,
    Handoff,
    Hook,
    NotesWindow,
    Other,
}

/// One ordered block of an assistant message, for serializers that price or render blocks
/// independently. An empty parts slice means a flat legacy message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemPart<'a> {
    Text(Cow<'a, str>),
    Reasoning(Cow<'a, str>),
    RedactedReasoning,
    /// Index into the message's `assistant_tool_calls`.
    Call(usize),
}

impl ItemPart<'static> {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(Cow::Owned(text.into()))
    }

    pub fn reasoning(text: impl Into<String>) -> Self {
        Self::Reasoning(Cow::Owned(text.into()))
    }
}

/// Where an image in an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOrigin {
    User,
    Tool,
    ArchiveFrame,
    InlineFrame,
}

/// Facts about one image attached to an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageMeta {
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub detail: Option<ImageDetail>,
    pub origin: ImageOrigin,
}

/// What an entry is, beyond its provider role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemKind {
    /// An ordinary conversation message.
    #[default]
    Message,
    /// The active compaction summary. Preparation passes it on as the previous summary.
    Summary,
    /// A branch-return summary written when the user navigated back.
    BranchSummary,
    /// A harness-authored user-role note (reminder, child completion, wake). It starts a turn.
    Note,
}

/// How the provider finished the assistant response that produced an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stop {
    #[default]
    Complete,
    /// The provider hit its output limit.
    Truncated,
    Aborted,
    Error,
}

/// Facts about a tool-result entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolFacts {
    pub is_error: bool,
    /// The tool marked the result as carrying no information (for example zero matches).
    pub useless: bool,
    /// A reducer already replaced the result text.
    pub pruned: bool,
    /// For reads: `Some(true)` when the whole file was returned untruncated.
    pub read_complete: Option<bool>,
    /// The result is an internal recovery artifact (for example an archived source) that
    /// reducers must not touch.
    pub internal_artifact: bool,
}

/// One context entry as the compaction algorithms see it.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    pub message: &'a CompletionMessage,
    pub kind: ItemKind,
    /// Journal sequence of the producing event. Tool results share their assistant's sequence.
    pub seq: u64,
    /// Request id grouping the entries of one turn.
    pub turn: Option<&'a str>,
    /// Settled reasoning text attached to an assistant entry.
    pub reasoning: &'a [String],
    pub tool: ToolFacts,
    pub image_count: u32,
    /// Provider-priced tokens of the attached images.
    pub image_tokens: u32,
    /// Usage reported for the request that produced this assistant entry.
    pub usage: Option<&'a CompletionUsage>,
    pub stop: Stop,
    /// Wall-clock time of the producing event, in Unix milliseconds.
    pub at_ms: Option<u64>,
    /// Local token estimate from [`crate::tokens::estimate_item`].
    pub tokens: u32,
    /// Local estimate excluding opaque reasoning bytes and output-size uplift.
    pub tokens_floor: u32,
    pub origin: Origin,
    pub note_kind: Option<NoteKind>,
    /// Ordered assistant blocks; empty for flat messages.
    pub parts: &'a [ItemPart<'a>],
    pub images: &'a [ImageMeta],
    /// When a reducer replaced this tool result, in Unix milliseconds.
    pub pruned_at_ms: Option<u64>,
    /// Output tokens already elided from this assistant entry by earlier edits.
    pub elided_output_tokens: u32,
    /// Bumped by every mutating edit; never persisted.
    pub version: u32,
    /// The summary was produced by a hook rather than a summarizer.
    pub from_hook: bool,
    /// An original entry kept after the active summary.
    pub retained: bool,
}

impl<'a> Item<'a> {
    /// A plain complete message with every optional fact absent.
    pub fn new(message: &'a CompletionMessage) -> Self {
        Self {
            message,
            kind: ItemKind::Message,
            seq: 0,
            turn: None,
            reasoning: &[],
            tool: ToolFacts::default(),
            image_count: 0,
            image_tokens: 0,
            usage: None,
            stop: Stop::Complete,
            at_ms: None,
            tokens: 0,
            tokens_floor: 0,
            origin: Origin::User,
            note_kind: None,
            parts: &[],
            images: &[],
            pruned_at_ms: None,
            elided_output_tokens: 0,
            version: 0,
            from_hook: false,
            retained: false,
        }
    }
}

impl Item<'_> {
    pub fn role(&self) -> MessageRole {
        self.message.role
    }

    pub fn text(&self) -> &str {
        &self.message.content
    }

    pub fn is_tool_result(&self) -> bool {
        self.message.role == MessageRole::Tool
    }

    pub fn is_assistant(&self) -> bool {
        self.message.role == MessageRole::Assistant
    }

    pub fn is_user(&self) -> bool {
        self.message.role == MessageRole::User
    }
}
