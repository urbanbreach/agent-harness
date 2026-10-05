use super::*;
use harness_core::event::ProviderRequestRetryMetadata;
use std::time::Duration;

use crate::app::{ToolCallPresentation, ToolCallPresentationStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptBlockPlacement {
    Flow,
    StickyPromptCandidate,
    PinnedFooter { outdent_cells: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TranscriptToolFamily {
    Unknown,
    Read,
    Search,
    List,
    Execute,
    Edit,
    Web,
    Task,
    Permission,
    Question,
}

pub(in crate::ui::ui_transcript) fn tool_family(
    tool: &TranscriptToolCallSection,
) -> TranscriptToolFamily {
    if tool
        .detail_blocks
        .iter()
        .any(|block| matches!(block, TranscriptToolCallDetailBlock::StructuredDiff { .. }))
    {
        return TranscriptToolFamily::Edit;
    }
    match tool.header.tool_id.as_str() {
        "question" | "user.question" => TranscriptToolFamily::Question,
        "apply_patch"
        | "edit"
        | "write"
        | "fs.write"
        | "edit.hashline_apply"
        | "ast_grep_replace"
        | "lsp.rename" => TranscriptToolFamily::Edit,
        tool_id => TranscriptToolVerb::from_tool_id(tool_id).map_or_else(
            || {
                if tool.header.presentation.status == ToolCallPresentationStatus::Waiting {
                    TranscriptToolFamily::Permission
                } else {
                    TranscriptToolFamily::Unknown
                }
            },
            |verb| match verb {
                TranscriptToolVerb::Run => TranscriptToolFamily::Execute,
                TranscriptToolVerb::Read | TranscriptToolVerb::Skill => TranscriptToolFamily::Read,
                TranscriptToolVerb::Search => TranscriptToolFamily::Search,
                TranscriptToolVerb::List => TranscriptToolFamily::List,
                TranscriptToolVerb::WebFetch | TranscriptToolVerb::WebSearch => {
                    TranscriptToolFamily::Web
                }
                TranscriptToolVerb::Subagent => TranscriptToolFamily::Task,
                TranscriptToolVerb::Edit => TranscriptToolFamily::Edit,
                TranscriptToolVerb::Mcp
                | TranscriptToolVerb::Message
                | TranscriptToolVerb::Other => TranscriptToolFamily::Unknown,
            },
        ),
    }
}

pub(super) const THINKING_TRACE_LABEL: &str = "Thinking:";

#[derive(Debug, Clone)]
pub(in crate::ui) struct TranscriptVisualEntryDraft {
    pub(in crate::ui) kind: TranscriptRenderSurfaceKind,
    pub(in crate::ui) leading_gap_rows: usize,
    pub(in crate::ui) trailing_gap_rows: usize,
    pub(in crate::ui) placement: TranscriptBlockPlacement,
    pub(in crate::ui) show_outer_rail: bool,
    pub(in crate::ui) rail_glyph: &'static str,
    pub(in crate::ui) rail_color: Color,
    pub(in crate::ui) surface: Color,
    pub(in crate::ui) lines: Vec<Line<'static>>,
    pub(in crate::ui) interaction_rows: Option<Vec<Option<TranscriptInteractionRow>>>,
    pub(in crate::ui) selection_rows: Option<Vec<SelectionRow>>,
    pub(in crate::ui) diff_hunk_offsets: Vec<usize>,
    pub(in crate::ui) selected_rail: bool,
    pub(in crate::ui) tool_rail_motion: Option<ToolRailMotion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolRailMotion {
    Running {
        elapsed: Duration,
        sampled_phase: usize,
    },
    Waiting,
    Queued,
    FinishFlash {
        elapsed: Duration,
        sampled_phase: usize,
    },
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolVerb {
    Run,
    Read,
    Search,
    List,
    Skill,
    WebFetch,
    WebSearch,
    Subagent,
    Edit,
    Mcp,
    Message,
    Other,
}

impl TranscriptToolVerb {
    pub(super) fn from_tool_id(tool_id: &str) -> Option<Self> {
        match tool_id {
            "shell.run" | "bash" => Some(Self::Run),
            "fs.read" | "read" | "session_read" => Some(Self::Read),
            "fs.glob" | "glob" | "fs.grep" | "grep" | "search.code" | "codesearch"
            | "session_search" | "ast_grep_search" | "lsp" | "code.lsp" => Some(Self::Search),
            "fs.ls" | "list" | "session_list" => Some(Self::List),
            "skill" | "skill.load" => Some(Self::Skill),
            "web.fetch" | "webfetch" => Some(Self::WebFetch),
            "search.web" | "websearch" => Some(Self::WebSearch),
            "spawn_subagent" | "agent.spawn" | "task" => Some(Self::Subagent),
            _ => Self::from_mcp_context_id(tool_id),
        }
    }

    fn from_mcp_context_id(tool_id: &str) -> Option<Self> {
        let (_, operation) = tool_id.strip_prefix("mcp.")?.split_once('.')?;
        match operation {
            "tools.list" | "resources.list" | "prompts.list" => Some(Self::List),
            "resource.read" | "prompt.get" => Some(Self::Read),
            _ => None,
        }
    }

    pub(super) fn from_tool_call(tool_call: &TranscriptToolCallSection) -> Option<Self> {
        let id = tool_call.header.tool_id.as_str();
        if id == "background.notification" && tool_call.header.subtitle.is_none() {
            return Some(Self::Subagent);
        }
        let verb = Self::from_tool_id(id).unwrap_or_else(|| match id {
            "apply_patch"
            | "edit"
            | "write"
            | "fs.write"
            | "edit.hashline_apply"
            | "ast_grep_replace"
            | "lsp.rename" => Self::Edit,
            "agent.message" | "agent.send_message" => Self::Message,
            _ if is_mcp_tool_id(id) => Self::Mcp,
            _ => Self::Other,
        });
        if verb == Self::Read
            && (tool_call.header.title == "Skill"
                || tool_call
                    .header
                    .path_metadata
                    .as_deref()
                    .is_some_and(|path| path.rsplit('/').next() == Some("SKILL.md")))
        {
            Some(Self::Skill)
        } else {
            Some(verb)
        }
    }

    pub(super) const fn group_kind(self) -> TranscriptToolGroupKind {
        match self {
            Self::Run | Self::Edit | Self::Mcp | Self::Message | Self::Other => {
                TranscriptToolGroupKind::Commands
            }
            Self::Read
            | Self::Search
            | Self::List
            | Self::Skill
            | Self::WebFetch
            | Self::WebSearch
            | Self::Subagent => TranscriptToolGroupKind::Context,
        }
    }

    const fn verb(self, running: bool) -> &'static str {
        let (settled, active) = match self {
            Self::Run | Self::Subagent | Self::Other => ("Ran", "Running"),
            Self::Read | Self::Skill => ("Read", "Reading"),
            Self::Search | Self::WebSearch => ("Searched", "Searching"),
            Self::List => ("Listed", "Listing"),
            Self::WebFetch => ("Fetched", "Fetching"),
            Self::Edit => ("Edited", "Editing"),
            Self::Mcp => ("Called", "Calling"),
            Self::Message => ("Sent", "Sending"),
        };
        if running {
            active
        } else {
            settled
        }
    }

    const fn noun(self, count: usize) -> &'static str {
        let (singular, plural) = match self {
            Self::Run => ("command", "commands"),
            Self::Read | Self::Edit => ("file", "files"),
            Self::Search => ("pattern", "patterns"),
            Self::List => ("dir", "dirs"),
            Self::Skill => ("skill", "skills"),
            Self::WebFetch | Self::WebSearch => ("website", "websites"),
            Self::Subagent => ("subagent", "subagents"),
            Self::Mcp => ("MCP tool", "MCP tools"),
            Self::Message => ("message", "messages"),
            Self::Other => ("tool", "tools"),
        };
        if count == 1 {
            singular
        } else {
            plural
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptToolVerbCount {
    verb: TranscriptToolVerb,
    count: usize,
    active_count: usize,
    running_sources: std::collections::BTreeSet<String>,
    sources: std::collections::BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolGroupKind {
    Commands,
    Context,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranscriptToolDisclosureMode {
    Collapsed,
    Preview,
    Expanded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolGroupSummary {
    pub(super) kind: TranscriptToolGroupKind,
    pub(super) span_len: usize,
    pub(super) member_count: usize,
    pub(super) verbs: Vec<TranscriptToolVerb>,
    verb_counts: Vec<TranscriptToolVerbCount>,
    pub(super) queued_count: usize,
    pub(super) running_count: usize,
    pub(super) waiting_count: usize,
    pub(super) succeeded_count: usize,
    pub(super) failed_count: usize,
    pub(super) disclosure: TranscriptToolDisclosureMode,
    pub(super) duration_ms: Option<u64>,
    pub(super) result_count: Option<u64>,
}

impl TranscriptToolGroupSummary {
    #[cfg(test)]
    pub(super) fn from_adjacent(parts: &[TranscriptAssistantPart]) -> Option<Self> {
        let first = parts.first()?.tool_call()?;
        let mut summary = Self::empty(first)?;

        for part in parts {
            let Some(tool_call) = part.tool_call() else {
                break;
            };
            if TranscriptToolVerb::from_tool_call(tool_call)?.group_kind() != summary.kind
                || !summary.push(tool_call)
            {
                break;
            }
            summary.span_len = summary.span_len.saturating_add(1);
        }

        (summary.member_count > 0).then_some(summary)
    }

    pub(super) fn from_tool_calls(tool_calls: &[&TranscriptToolCallSection]) -> Option<Self> {
        let first = *tool_calls.first()?;
        let mut summary = Self::empty(first)?;
        for tool_call in tool_calls {
            if !summary.push(tool_call) {
                break;
            }
            summary.span_len = summary.span_len.saturating_add(1);
        }
        (summary.member_count > 0).then_some(summary)
    }

    fn empty(first: &TranscriptToolCallSection) -> Option<Self> {
        let first_verb = TranscriptToolVerb::from_tool_call(first)?;
        Some(Self {
            kind: first_verb.group_kind(),
            span_len: 0,
            member_count: 0,
            verbs: Vec::new(),
            verb_counts: Vec::new(),
            queued_count: 0,
            running_count: 0,
            waiting_count: 0,
            succeeded_count: 0,
            failed_count: 0,
            disclosure: TranscriptToolDisclosureMode::Collapsed,
            duration_ms: None,
            result_count: None,
        })
    }

    fn push(&mut self, tool_call: &TranscriptToolCallSection) -> bool {
        if tool_call.header.presentation.status == ToolCallPresentationStatus::Waiting {
            return false;
        }
        let Some(verb) = TranscriptToolVerb::from_tool_call(tool_call) else {
            return false;
        };
        self.member_count += 1;
        if let Some(bucket) = self
            .verb_counts
            .iter_mut()
            .find(|bucket| bucket.verb == verb)
        {
            bucket.count += 1;
        } else {
            self.verbs.push(verb);
            self.verb_counts.push(TranscriptToolVerbCount {
                verb,
                count: 1,
                active_count: 0,
                running_sources: std::collections::BTreeSet::new(),
                sources: std::collections::BTreeSet::new(),
            });
        }
        if let Some(bucket) = self
            .verb_counts
            .iter_mut()
            .find(|bucket| bucket.verb == verb)
        {
            if !tool_call.cancellation_requested
                && matches!(
                    tool_call.header.presentation.status,
                    ToolCallPresentationStatus::Queued | ToolCallPresentationStatus::Running
                )
            {
                bucket.active_count += 1;
                bucket
                    .running_sources
                    .extend(tool_call.group.sources.iter().cloned());
            }
            bucket
                .sources
                .extend(tool_call.group.sources.iter().cloned());
        }
        match tool_call.header.presentation.status {
            _ if tool_call.cancellation_requested => {}
            ToolCallPresentationStatus::Queued => self.queued_count += 1,
            ToolCallPresentationStatus::Running => self.running_count += 1,
            ToolCallPresentationStatus::Waiting => return false,
            ToolCallPresentationStatus::Succeeded => self.succeeded_count += 1,
            ToolCallPresentationStatus::Failed => self.failed_count += 1,
            ToolCallPresentationStatus::Cancelled => {}
        }
        self.disclosure = if tool_call.expanded {
            TranscriptToolDisclosureMode::Expanded
        } else if self.disclosure != TranscriptToolDisclosureMode::Expanded
            && tool_call.details_preview_visible
        {
            TranscriptToolDisclosureMode::Preview
        } else {
            self.disclosure
        };
        if let Some(duration_ms) = tool_call.header.presentation.duration_ms {
            self.duration_ms = Some(
                self.duration_ms
                    .unwrap_or_default()
                    .saturating_add(duration_ms),
            );
        }
        if let Some(result_count) = tool_call.header.presentation.result_count {
            self.result_count = Some(
                self.result_count
                    .unwrap_or_default()
                    .saturating_add(result_count),
            );
        }
        true
    }

    #[cfg(test)]
    pub(super) const fn folds_as_group(&self) -> bool {
        match self.kind {
            TranscriptToolGroupKind::Commands => self.member_count > 11,
            TranscriptToolGroupKind::Context => self.member_count > 0,
        }
    }

    pub(super) fn semantic_label(&self) -> String {
        let mut label = self.semantic_core_label();
        if self.failed_count > 0 {
            label.push_str(&format!(" · {} failed", self.failed_count));
        }
        label
    }

    pub(super) fn semantic_core_label(&self) -> String {
        let running = self.running_count > 0 || self.queued_count > 0;
        self.verb_counts
            .iter()
            .map(|bucket| {
                let count = if bucket.sources.is_empty() {
                    bucket.count
                } else {
                    bucket.sources.len()
                };
                if bucket.verb == TranscriptToolVerb::Subagent {
                    let running = if bucket.sources.is_empty() {
                        bucket.active_count
                    } else {
                        bucket.running_sources.len()
                    };
                    let (verb, active) = if running > 0 {
                        ("Running", running)
                    } else {
                        ("Ran", count)
                    };
                    let mut label = format!("{verb} {active} {}", bucket.verb.noun(active));
                    if running > 0 && running < count {
                        label.push_str(&format!(", {} completed", count - running));
                    }
                    return label;
                }
                format!(
                    "{} {} {}",
                    bucket.verb.verb(running),
                    count,
                    bucket.verb.noun(count)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranscriptRenderSurfaceKind {
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

#[derive(Debug, Clone)]
pub(super) struct TranscriptOrderedToolCallSection {
    pub(super) tool_call_id: String,
    pub(super) first_seq: u64,
    pub(super) section: TranscriptToolCallSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct TranscriptAssistantPartSourceId(pub(super) u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptTurnSection {
    pub(super) raw_parts: Vec<usize>,
    pub(super) child_view: bool,
    pub(super) activity_first_seq: u64,
    pub(super) request_id: String,
    pub(super) user_message: Option<TranscriptUserMessageSection>,
    pub(super) show_footer: bool,
    pub(super) footer_timestamp: Option<String>,
    pub(super) animation_phase: usize,
    pub(super) motion_enabled: bool,
    pub(super) reasoning_expanded: bool,
    pub(super) header: TranscriptTurnHeader,
    pub(super) assistant_parts: Vec<TranscriptAssistantPart>,
    pub(super) assistant_part_source_ids: Vec<TranscriptAssistantPartSourceId>,
}

impl TranscriptTurnSection {
    pub(super) fn reasoning_active(&self, part_index: usize) -> bool {
        self.header.status == ActivityStatus::Streaming
            && part_index + 1 == self.assistant_parts.len()
            && matches!(self.assistant_parts.get(part_index),
                Some(TranscriptAssistantPart::Reasoning(reasoning)) if reasoning.duration_ms.is_none())
            && !self.assistant_tools().any(|tool| {
                matches!(tool.header.tool_id.as_str(), "question" | "user.question")
                    || tool.header.presentation.status == ToolCallPresentationStatus::Waiting
            })
    }

    pub(super) fn assistant_tools(&self) -> impl Iterator<Item = &TranscriptToolCallSection> {
        self.assistant_parts.iter().filter_map(|part| match part {
            TranscriptAssistantPart::ToolCall(tool) => Some(tool.as_ref()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptUserMessageSection {
    pub(super) text: String,
    pub(super) queued: bool,
    pub(super) wall_clock: Option<String>,
    pub(super) expanded_wall_clock: Option<String>,
    pub(super) wall_clock_hovered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptTurnHeader {
    pub(super) status: ActivityStatus,
    pub(super) is_selected: bool,
    pub(super) is_hovered: bool,
    pub(super) provider_request_open: bool,
    pub(super) profile_label: String,
    pub(super) model_id: String,
    pub(super) duration_ms: Option<u64>,
    /// Reasoning-only mono span for "Thought for" (waiting-state packing).
    pub(super) thinking_duration_ms: Option<u64>,
    /// Elapsed time since first stream delta for "Responding…" (waiting-state packing).
    pub(super) responding_duration_ms: Option<u64>,
    pub(super) total_tokens: Option<u32>,
    pub(super) retry: Option<ProviderRequestRetryMetadata>,
    pub(super) retry_elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TranscriptBodyBlock {
    RichText(String),
    StreamingRichText(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptLabeledTextSection {
    pub(super) label: &'static str,
    pub(super) text: String,
    pub(super) duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallSection {
    pub(super) hook_executions: Vec<harness_core::event::HookExecutionMetadata>,
    pub(super) tool_call_id: String,
    pub(super) coalesced_tool_call_ids: Vec<String>,
    pub(super) child_session_id: Option<String>,
    pub(super) subagent_background: bool,
    pub(super) output_truncated: bool,
    pub(super) replay_read_only: bool,
    pub(super) hovered_target: Option<TranscriptMouseTarget>,
    pub(super) header: TranscriptToolCallHeader,
    pub(super) detail_blocks: Vec<TranscriptToolCallDetailBlock>,
    pub(super) details_collapsed_by_default: bool,
    pub(super) details_preview_visible: bool,
    pub(super) animation_phase: usize,
    pub(super) expanded: bool,
    pub(super) rail_motion: ToolRailMotion,
    pub(super) cancellation_requested: bool,
    pub(super) group: TranscriptToolGroupMember,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TranscriptToolGroupMember {
    pub(super) expanded: bool,
    pub(super) sources: std::collections::BTreeSet<String>,
}

impl TranscriptToolCallSection {
    pub(super) fn details_visible(&self) -> bool {
        !self.details_collapsed_by_default || self.details_preview_visible || self.expanded
    }

    pub(super) fn has_detail_content(&self) -> bool {
        !self.detail_blocks.is_empty()
            || self
                .hook_executions
                .iter()
                .any(|hook| hook.status != harness_core::event::HookExecutionStatus::Skipped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallHeader {
    pub(super) selected: bool,
    pub(super) tool_id: String,
    pub(super) title: String,
    pub(super) subtitle: Option<String>,
    pub(super) path_metadata: Option<String>,
    pub(super) icon: Option<&'static str>,
    pub(super) presentation: ToolCallPresentation,
    pub(super) visual_style: TranscriptToolCallVisualStyle,
    pub(super) struck_out: bool,
    pub(super) disclosure_state: Option<TranscriptToolCallDisclosureState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolCallDetailBlock {
    Recorded(super::super::ui_recorded_tool_output::RecordedToolOutput),
    ReadOutput {
        text: String,
        start_line: Option<u64>,
    },
    Message {
        text: String,
        tone: TranscriptToolCallDetailTone,
    },
    InputPreview {
        text: String,
        language: String,
    },
    EvalPanel {
        code: String,
        language: String,
        output: String,
        failed: bool,
    },
    BashPanel {
        command: String,
        output: String,
        description: Option<String>,
    },
    StructuredDiff {
        diff_content: String,
        before_source: Option<String>,
        fallback_path: Option<String>,
        force_stacked: bool,
        plain_numbered: bool,
        highlight_syntax: bool,
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
    pub(super) expanded: bool,
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

impl TranscriptAssistantPart {
    pub(super) fn tool_call(&self) -> Option<&TranscriptToolCallSection> {
        match self {
            Self::ToolCall(tool_call) => Some(tool_call),
            Self::Reasoning(_) | Self::Body(_) | Self::Error(_) | Self::Compaction(_) => None,
        }
    }
}

// the reference application: HorizontalLayout::ACCENT (1) + LayoutConfig::block_pad_left (2).
// Every entry keeps this origin through streaming, folding, selection and settlement.
pub(super) const TRANSCRIPT_ASSISTANT_BODY_PREFIX: &str =
    super::super::ui_transcript_surface::TRANSCRIPT_ENTRY_CONTENT_PREFIX;
pub(super) const TRANSCRIPT_USER_BODY_PREFIX: &str = "     ";
pub(super) const TRANSCRIPT_NESTED_INDENT: &str = "     ";
pub(super) const TRANSCRIPT_TOOL_BODY_PREFIX: &str = TRANSCRIPT_ASSISTANT_BODY_PREFIX;

#[cfg(test)]
mod tool_group_tests {
    use super::*;
    use crate::app::{ToolCallDisplayStatus, ToolCallPresentation};

    fn tool_part(
        id: &str,
        tool_id: &str,
        status: ToolCallDisplayStatus,
        preview: bool,
        expanded: bool,
    ) -> TranscriptAssistantPart {
        TranscriptAssistantPart::ToolCall(Box::new(TranscriptToolCallSection {
            group: Default::default(),
            hook_executions: Vec::new(),
            tool_call_id: id.to_string(),
            coalesced_tool_call_ids: vec![id.to_string()],
            child_session_id: None,
            subagent_background: false,
            output_truncated: false,
            replay_read_only: false,
            hovered_target: None,
            header: TranscriptToolCallHeader {
                selected: false,
                tool_id: tool_id.to_string(),
                title: tool_id.to_string(),
                subtitle: None,
                path_metadata: None,
                icon: None,
                presentation: ToolCallPresentation::from_display_status(status),
                visual_style: TranscriptToolCallVisualStyle::Inline,
                struck_out: false,
                disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
            },
            detail_blocks: Vec::new(),
            details_collapsed_by_default: true,
            details_preview_visible: preview,
            animation_phase: 0,
            expanded,
            rail_motion: ToolRailMotion::Settled,
            cancellation_requested: false,
        }))
    }

    #[test]
    fn adjacent_command_group_keeps_mixed_member_states() {
        // arrange
        let parts = vec![
            tool_part(
                "success",
                "bash",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
            tool_part(
                "running",
                "shell.run",
                ToolCallDisplayStatus::Running,
                false,
                false,
            ),
            tool_part(
                "failure",
                "bash",
                ToolCallDisplayStatus::Failed,
                false,
                false,
            ),
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText("stop".to_string())),
        ];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("command group");

        // assert
        assert_eq!(summary.kind, TranscriptToolGroupKind::Commands);
        assert_eq!(summary.member_count, 3);
        assert_eq!(summary.succeeded_count, 1);
        assert_eq!(summary.running_count, 1);
        assert_eq!(summary.failed_count, 1);
    }

    #[test]
    fn single_command_stays_individual() {
        // arrange
        let parts = vec![tool_part(
            "command",
            "bash",
            ToolCallDisplayStatus::Succeeded,
            false,
            false,
        )];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("command group");

        // assert
        assert_eq!(summary.kind, TranscriptToolGroupKind::Commands);
        assert!(!summary.folds_as_group());
    }

    #[test]
    fn adjacent_group_stops_at_typed_group_boundary() {
        // arrange
        let parts = vec![
            tool_part(
                "read",
                "read",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
            tool_part(
                "search",
                "grep",
                ToolCallDisplayStatus::Running,
                false,
                false,
            ),
            tool_part(
                "command",
                "bash",
                ToolCallDisplayStatus::Running,
                false,
                false,
            ),
        ];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        // assert
        assert_eq!(summary.kind, TranscriptToolGroupKind::Context);
        assert_eq!(summary.member_count, 2);
        assert_eq!(
            summary.verbs,
            vec![TranscriptToolVerb::Read, TranscriptToolVerb::Search]
        );
    }

    #[test]
    fn context_group_label_uses_present_tense_for_every_bucket_while_running() {
        // arrange
        let parts = vec![
            tool_part(
                "read",
                "session_read",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
            tool_part(
                "search",
                "ast_grep_search",
                ToolCallDisplayStatus::Running,
                false,
                false,
            ),
        ];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        // assert
        assert_eq!(
            summary.semantic_label(),
            "Reading 1 file, Searching 1 pattern"
        );
    }

    #[test]
    fn context_group_label_appends_failed_member_count() {
        // arrange
        let parts = vec![
            tool_part(
                "failed",
                "web.fetch",
                ToolCallDisplayStatus::Failed,
                false,
                false,
            ),
            tool_part(
                "succeeded",
                "search.web",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
        ];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        // assert
        assert_eq!(
            summary.semantic_label(),
            "Fetched 1 website, Searched 1 website · 1 failed"
        );
    }

    #[test]
    fn skill_file_read_uses_the_skill_bucket() {
        // arrange
        let mut part = tool_part(
            "skill-read",
            "read",
            ToolCallDisplayStatus::Succeeded,
            false,
            false,
        );
        let TranscriptAssistantPart::ToolCall(tool) = &mut part else {
            panic!("tool")
        };
        tool.header.path_metadata = Some(".agent-harness/skills/harness-qa/SKILL.md".into());

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&[part]).expect("context group");

        // assert
        assert_eq!(summary.semantic_label(), "Read 1 skill");
    }

    #[test]
    fn waiting_context_tool_stays_outside_verb_group() {
        // arrange
        let parts = vec![
            tool_part(
                "read",
                "read",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
            tool_part(
                "waiting",
                "grep",
                ToolCallDisplayStatus::PendingPermission,
                false,
                false,
            ),
        ];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        // assert
        assert_eq!(summary.member_count, 1);
    }

    #[test]
    fn group_disclosure_promotes_collapsed_to_preview_then_expanded() {
        // arrange
        // act
        let collapsed = vec![
            tool_part(
                "one",
                "read",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
            tool_part(
                "two",
                "grep",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
        ];
        let preview = vec![
            tool_part("one", "read", ToolCallDisplayStatus::Succeeded, true, false),
            tool_part(
                "two",
                "grep",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
        ];
        let expanded = vec![
            tool_part("one", "read", ToolCallDisplayStatus::Succeeded, true, false),
            tool_part("two", "grep", ToolCallDisplayStatus::Succeeded, false, true),
        ];

        // assert
        assert_eq!(
            TranscriptToolGroupSummary::from_adjacent(&collapsed)
                .expect("collapsed")
                .disclosure,
            TranscriptToolDisclosureMode::Collapsed
        );
        assert_eq!(
            TranscriptToolGroupSummary::from_adjacent(&preview)
                .expect("preview")
                .disclosure,
            TranscriptToolDisclosureMode::Preview
        );
        assert_eq!(
            TranscriptToolGroupSummary::from_adjacent(&expanded)
                .expect("expanded")
                .disclosure,
            TranscriptToolDisclosureMode::Expanded
        );
    }

    #[test]
    fn completed_tool_sections_default_to_settled_motion() {
        // arrange
        // act
        let part = tool_part(
            "completed",
            "read",
            ToolCallDisplayStatus::Succeeded,
            false,
            false,
        );

        // assert
        assert!(matches!(
            part,
            TranscriptAssistantPart::ToolCall(section)
                if section.rail_motion == ToolRailMotion::Settled
        ));
    }
}
