use super::*;
use harness_core::event::ProviderRequestRetryMetadata;

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptToolCardShell {
    pub(super) indent: &'static str,
    pub(super) rail_color: Color,
    pub(super) surface: Color,
    pub(super) content_leading_spaces: &'static str,
}

pub(super) const THINKING_TRACE_LABEL: &str = "Thinking:";

#[derive(Debug)]
pub(super) struct TranscriptLayoutCacheEntry {
    pub(super) app_instance_id: u64,
    pub(super) render_key: u64,
    pub(super) theme: Theme,
    pub(super) width: u16,
    pub(super) base_surface: Color,
    pub(super) layout: MeasuredTranscriptLayout,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct TranscriptRenderSurface {
    pub(in crate::ui) kind: TranscriptRenderSurfaceKind,
    pub(in crate::ui) show_outer_rail: bool,
    pub(in crate::ui) rail_glyph: &'static str,
    pub(in crate::ui) rail_color: Color,
    pub(in crate::ui) surface: Color,
    pub(in crate::ui) lines: Vec<Line<'static>>,
    pub(in crate::ui) interaction_rows: Option<Vec<Option<TranscriptInteractionRow>>>,
    pub(in crate::ui) selection_rows: Option<Vec<TranscriptSelectionRow>>,
    pub(in crate::ui) diff_hunk_offsets: Vec<usize>,
    pub(in crate::ui) selected_rail: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptRenderSurfaceKind {
    User,
    AssistantReasoning,
    AssistantBody,
    AssistantTool,
    AssistantCommandTool,
    AssistantError,
    AssistantFooter,
    Compaction,
}

#[derive(Debug, Clone)]
pub(super) struct ToolSectionRender {
    pub(super) lines: Vec<Line<'static>>,
    pub(super) interaction_rows: Vec<Option<TranscriptInteractionRow>>,
    pub(super) diff_hunk_offsets: Vec<usize>,
}

pub(super) struct BuildTurnSectionArgs<'a> {
    pub(super) activity: &'a ActivityEntry,
    pub(super) queued_user_message: bool,
    pub(super) is_selected: bool,
    pub(super) is_latest: bool,
    pub(super) thinking_visible: bool,
    pub(super) timestamps_visible: bool,
    pub(super) show_tool_details: bool,
    pub(super) show_generic_tool_output: bool,
    pub(super) stacked_diffs: bool,
    pub(super) session_path: Option<&'a Path>,
    pub(super) app: &'a AppState,
}

#[derive(Debug, Clone)]
pub(super) struct TranscriptOrderedToolCallSection {
    pub(super) tool_call_id: String,
    pub(super) first_seq: u64,
    pub(super) section: TranscriptToolCallSection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptTurnSection {
    pub(super) request_id: String,
    pub(super) user_message: Option<TranscriptUserMessageSection>,
    pub(super) show_footer: bool,
    pub(super) footer_timestamp: Option<String>,
    pub(super) animation_phase: usize,
    pub(super) header: TranscriptTurnHeader,
    pub(super) body_blocks: Vec<TranscriptBodyBlock>,
    pub(super) tool_calls: Vec<TranscriptToolCallSection>,
    pub(super) thinking: Option<TranscriptLabeledTextSection>,
    pub(super) error: Option<TranscriptErrorSection>,
    pub(super) assistant_parts: Vec<TranscriptAssistantPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptUserMessageSection {
    pub(super) text: String,
    pub(super) queued: bool,
    pub(super) wall_clock: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptTurnHeader {
    pub(super) status: ActivityStatus,
    pub(super) is_selected: bool,
    pub(super) profile_label: String,
    pub(super) model_id: String,
    pub(super) duration_ms: Option<u64>,
    /// Reasoning-only mono span for "Thought for" (Grok freeze packing).
    pub(super) thinking_duration_ms: Option<u64>,
    /// Elapsed time since first stream delta for "Responding…" (Grok freeze packing).
    pub(super) responding_duration_ms: Option<u64>,
    pub(super) total_tokens: Option<u32>,
    pub(super) retry: Option<ProviderRequestRetryMetadata>,
    pub(super) retry_elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TranscriptBodyBlock {
    RichText(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptLabeledTextSection {
    pub(super) label: &'static str,
    pub(super) text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallSection {
    pub(super) tool_call_id: String,
    pub(super) child_session_id: Option<String>,
    pub(super) hovered_target: Option<TranscriptMouseTarget>,
    pub(super) header: TranscriptToolCallHeader,
    pub(super) detail_blocks: Vec<TranscriptToolCallDetailBlock>,
    pub(super) expanded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallHeader {
    pub(super) tool_id: String,
    pub(super) title: String,
    pub(super) subtitle: Option<String>,
    pub(super) path_metadata: Option<String>,
    pub(super) icon: Option<&'static str>,
    pub(super) status: ToolCallDisplayStatus,
    pub(super) visual_style: TranscriptToolCallVisualStyle,
    pub(super) struck_out: bool,
    pub(super) disclosure_state: Option<TranscriptToolCallDisclosureState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolCallDetailBlock {
    Message {
        text: String,
        tone: TranscriptToolCallDetailTone,
    },
    Markdown {
        text: String,
    },
    TodoList {
        items: Vec<TranscriptTodoItem>,
    },
    BashPanel {
        command: String,
        output: String,
        description: Option<String>,
        expand_hint: Option<String>,
        tone: TranscriptToolCallDetailTone,
    },
    StructuredDiff {
        diff_content: String,
        fallback_path: Option<String>,
        force_stacked: bool,
        plain_numbered: bool,
        show_file_header: bool,
    },
    FileSection(TranscriptToolCallFileSection),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptToolCallFileSection {
    pub(super) tool_call_id: String,
    pub(super) file_path: String,
    pub(super) title: String,
    pub(super) subtitle: Option<String>,
    pub(super) disclosure_state: TranscriptToolCallDisclosureState,
    pub(in crate::ui) detail_blocks: Vec<TranscriptToolCallDetailBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolCallDetailTone {
    Primary,
    Secondary,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptErrorSection {
    pub(super) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptCompactionKind {
    SessionCompaction,
    BranchSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptCompactionSection {
    pub(super) kind: TranscriptCompactionKind,
    pub(super) summary: String,
    pub(super) tokens_before: Option<u32>,
    pub(super) read_files: Vec<String>,
    pub(super) modified_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TranscriptAssistantPart {
    Reasoning(TranscriptLabeledTextSection),
    Body(TranscriptBodyBlock),
    ToolCall(Box<TranscriptToolCallSection>),
    Error(TranscriptErrorSection),
    Compaction(TranscriptCompactionSection),
}

pub(super) const TRANSCRIPT_ASSISTANT_BODY_PREFIX: &str = "   ";
pub(super) const TRANSCRIPT_USER_BODY_PREFIX: &str = "     ";
pub(super) const TRANSCRIPT_REASONING_BODY_PREFIX: &str = "   ";
pub(super) const TRANSCRIPT_REASONING_HEADER_PREFIX: &str = "   ";
pub(super) const TRANSCRIPT_SELECTED_RAIL_GLYPH: &str = "❙";
pub(super) const TRANSCRIPT_NESTED_INDENT: &str = "     ";
pub(super) const TRANSCRIPT_OPCODE_EDIT_INDENT: &str = "       ";
