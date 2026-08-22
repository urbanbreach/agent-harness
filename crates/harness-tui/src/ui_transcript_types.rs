use super::ui_transcript_block_grammar::TranscriptBlockSpec;
use super::*;
use harness_core::event::ProviderRequestRetryMetadata;
use std::time::Duration;

use crate::app::{ToolCallPresentation, ToolCallPresentationStatus};

#[derive(Debug, Clone, Copy)]
pub(super) struct TranscriptToolCardShell {
    pub(super) indent: &'static str,
    pub(super) rail_color: Color,
    pub(super) surface: Color,
    pub(super) content_leading_spaces: &'static str,
}

pub(super) const THINKING_TRACE_LABEL: &str = "Thinking:";

#[derive(Debug, Clone)]
pub(super) struct TranscriptLayoutCacheEntry {
    pub(super) app_instance_id: u64,
    pub(super) render_key: u64,
    pub(super) theme: Theme,
    pub(super) width: u16,
    pub(super) base_surface: Color,
    pub(super) sections: Vec<TranscriptTurnSection>,
    pub(super) normalized_specs: Vec<Vec<TranscriptBlockSpec>>,
    pub(super) layout: MeasuredTranscriptLayout,
}

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
    pub(in crate::ui) selection_rows: Option<Vec<TranscriptSelectionRow>>,
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
            "agent.spawn" | "task" | "background_output" | "background_cancel" => {
                Some(Self::Subagent)
            }
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

    fn from_tool_call(tool_call: &TranscriptToolCallSection) -> Option<Self> {
        let verb = Self::from_tool_id(&tool_call.header.tool_id)?;
        if verb == Self::Read
            && tool_call
                .header
                .path_metadata
                .as_deref()
                .is_some_and(|path| path.rsplit('/').next() == Some("SKILL.md"))
        {
            Some(Self::Skill)
        } else {
            Some(verb)
        }
    }

    pub(super) const fn group_kind(self) -> TranscriptToolGroupKind {
        match self {
            Self::Run => TranscriptToolGroupKind::Commands,
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
            Self::Run | Self::Subagent => ("Ran", "Running"),
            Self::Read | Self::Skill => ("Read", "Reading"),
            Self::Search | Self::WebSearch => ("Searched", "Searching"),
            Self::List => ("Listed", "Listing"),
            Self::WebFetch => ("Fetched", "Fetching"),
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
            Self::Read => ("file", "files"),
            Self::Search => ("pattern", "patterns"),
            Self::List => ("dir", "dirs"),
            Self::Skill => ("skill", "skills"),
            Self::WebFetch | Self::WebSearch => ("website", "websites"),
            Self::Subagent => ("subagent", "subagents"),
        };
        if count == 1 {
            singular
        } else {
            plural
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TranscriptToolVerbCount {
    verb: TranscriptToolVerb,
    count: usize,
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
    pub(super) cancelled_count: usize,
    pub(super) disclosure: TranscriptToolDisclosureMode,
    pub(super) duration_ms: Option<u64>,
    pub(super) result_count: Option<u64>,
}

impl TranscriptToolGroupSummary {
    pub(super) fn from_adjacent(parts: &[TranscriptAssistantPart]) -> Option<Self> {
        let first = parts.first()?.tool_call()?;
        let mut summary = Self::empty(first)?;

        for part in parts {
            if matches!(part, TranscriptAssistantPart::Reasoning(_)) && summary.member_count > 0 {
                summary.span_len = summary.span_len.saturating_add(1);
                continue;
            }
            let Some(tool_call) = part.tool_call() else {
                break;
            };
            if !summary.push(tool_call) {
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
            cancelled_count: 0,
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
        if verb.group_kind() != self.kind {
            return false;
        }

        self.member_count += 1;
        if let Some(bucket) = self
            .verb_counts
            .iter_mut()
            .find(|bucket| bucket.verb == verb)
        {
            bucket.count += 1;
        } else {
            self.verbs.push(verb);
            self.verb_counts
                .push(TranscriptToolVerbCount { verb, count: 1 });
        }
        match tool_call.header.presentation.status {
            ToolCallPresentationStatus::Queued => self.queued_count += 1,
            ToolCallPresentationStatus::Running => self.running_count += 1,
            ToolCallPresentationStatus::Waiting => return false,
            ToolCallPresentationStatus::Succeeded => self.succeeded_count += 1,
            ToolCallPresentationStatus::Failed => self.failed_count += 1,
            ToolCallPresentationStatus::Cancelled => self.cancelled_count += 1,
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

    pub(super) const fn folds_as_group(&self) -> bool {
        match self.kind {
            TranscriptToolGroupKind::Commands => self.member_count > 0,
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
                format!(
                    "{} {} {}",
                    bucket.verb.verb(running),
                    bucket.count,
                    bucket.verb.noun(bucket.count)
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

pub(super) struct BuildTurnSectionArgs<'a> {
    pub(super) activity_first_seq: u64,
    pub(super) activity: &'a ActivityEntry,
    pub(super) queued_user_message: bool,
    pub(super) is_selected: bool,
    pub(super) is_latest: bool,
    pub(super) thinking_visible: bool,
    pub(super) timestamps_visible: bool,
    pub(super) show_tool_details: bool,
    pub(super) show_generic_tool_output: bool,
    pub(super) stacked_diffs: bool,
    pub(super) motion_enabled: bool,
    pub(super) session_path: Option<&'a Path>,
    pub(super) app: &'a AppState,
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
    pub(super) fn assistant_tools(&self) -> impl Iterator<Item = &TranscriptToolCallSection> {
        self.assistant_parts.iter().filter_map(|part| match part {
            TranscriptAssistantPart::ToolCall(tool) => Some(tool.as_ref()),
            _ => None,
        })
    }

    pub(super) fn assistant_reasoning(&self) -> Option<&TranscriptLabeledTextSection> {
        self.assistant_parts.iter().find_map(|part| match part {
            TranscriptAssistantPart::Reasoning(reasoning) => Some(reasoning),
            _ => None,
        })
    }

    pub(super) fn assistant_bodies(&self) -> impl Iterator<Item = &TranscriptBodyBlock> {
        self.assistant_parts.iter().filter_map(|part| match part {
            TranscriptAssistantPart::Body(body) => Some(body),
            _ => None,
        })
    }

    pub(super) fn assistant_error(&self) -> Option<&TranscriptErrorSection> {
        self.assistant_parts.iter().find_map(|part| match part {
            TranscriptAssistantPart::Error(error) => Some(error),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallSection {
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
}

impl TranscriptToolCallSection {
    pub(super) fn details_visible(&self) -> bool {
        !self.details_collapsed_by_default || self.details_preview_visible || self.expanded
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptToolCallHeader {
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
    fn tool_call(&self) -> Option<&TranscriptToolCallSection> {
        match self {
            Self::ToolCall(tool_call) => Some(tool_call),
            Self::Reasoning(_) | Self::Body(_) | Self::Error(_) | Self::Compaction(_) => None,
        }
    }
}

pub(super) const TRANSCRIPT_ASSISTANT_BODY_PREFIX: &str = "   ";
pub(super) const TRANSCRIPT_USER_BODY_PREFIX: &str = "     ";
pub(super) const TRANSCRIPT_REASONING_BODY_PREFIX: &str = "   ";
pub(super) const TRANSCRIPT_REASONING_HEADER_PREFIX: &str = "   ";
pub(super) const TRANSCRIPT_NESTED_INDENT: &str = "     ";
pub(super) const TRANSCRIPT_OPCODE_EDIT_INDENT: &str = "       ";

#[cfg(test)]
mod tool_group_tests {
    use super::super::ui_transcript_block_grammar::{
        TranscriptBlockMotionDemand, TranscriptToolDisclosure, TranscriptToolPolicy,
        TranscriptToolStatus,
    };
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
            tool_call_id: id.to_string(),
            coalesced_tool_call_ids: vec![id.to_string()],
            child_session_id: None,
            subagent_background: false,
            output_truncated: false,
            replay_read_only: false,
            hovered_target: None,
            header: TranscriptToolCallHeader {
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
    fn single_command_folds_as_command_group() {
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
        assert!(summary.folds_as_group());
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

    #[test]
    fn completed_reasoning_is_transparent_inside_a_tool_group_span() {
        // arrange
        let parts = vec![
            tool_part(
                "read",
                "read",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: "Thought",
                text: "checked the result".to_string(),
            }),
            tool_part(
                "search",
                "grep",
                ToolCallDisplayStatus::Succeeded,
                false,
                false,
            ),
        ];

        // act
        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        // assert
        assert_eq!(summary.member_count, 2);
        assert_eq!(summary.span_len, 3);
    }

    #[test]
    fn transcript_block_spec_enumerates_semantic_families() {
        // arrange
        use super::super::ui_transcript_block_grammar::{
            test_spec, validate_block_spec, TranscriptBlockContent, TranscriptBlockId,
            TranscriptBlockRole, TranscriptFooterContent, TranscriptFooterLifecycle,
            TranscriptLifecycleState, TranscriptPromptState, TranscriptSubagentLifecycle,
            TranscriptSubagentMode, TranscriptSubagentPolicy, TranscriptToolFamily,
            TranscriptToolGroupClass,
        };

        // act
        let families = [
            TranscriptToolFamily::Unknown,
            TranscriptToolFamily::Group,
            TranscriptToolFamily::Read,
            TranscriptToolFamily::Search,
            TranscriptToolFamily::List,
            TranscriptToolFamily::Execute,
            TranscriptToolFamily::Edit,
            TranscriptToolFamily::Web,
            TranscriptToolFamily::Task,
            TranscriptToolFamily::Permission,
            TranscriptToolFamily::Question,
        ];
        let mut specs = vec![
            test_spec(
                TranscriptBlockRole::UserPrompt,
                TranscriptBlockContent::UserMessage {
                    text: String::new(),
                    queued: false,
                    wall_clock: None,
                    state: TranscriptPromptState::Idle,
                },
            ),
            test_spec(
                TranscriptBlockRole::AssistantBody,
                TranscriptBlockContent::AssistantBody {
                    text: String::new(),
                    streaming: false,
                    wall_clock: None,
                    has_tools: false,
                },
            ),
            test_spec(
                TranscriptBlockRole::Reasoning,
                TranscriptBlockContent::Reasoning {
                    text: String::new(),
                    active: false,
                    expanded: false,
                    duration_ms: None,
                    motion_enabled: false,
                },
            ),
            test_spec(
                TranscriptBlockRole::Footer,
                TranscriptBlockContent::Footer {
                    lifecycle: TranscriptFooterLifecycle::Settled,
                    state: TranscriptLifecycleState::Completed,
                    content: TranscriptFooterContent::Settled,
                },
            ),
            test_spec(
                TranscriptBlockRole::Error,
                TranscriptBlockContent::Error {
                    message: String::new(),
                },
            ),
            test_spec(
                TranscriptBlockRole::Compaction,
                TranscriptBlockContent::Compaction {
                    branch_summary: false,
                    summary: String::new(),
                    tokens_before: None,
                    read_files: Vec::new(),
                    modified_files: Vec::new(),
                },
            ),
            test_spec(
                TranscriptBlockRole::Synthetic,
                TranscriptBlockContent::Synthetic {
                    value: String::new(),
                },
            ),
        ];
        specs.extend(families.into_iter().map(|family| {
            let grouped = family == TranscriptToolFamily::Group;
            let subagent = family == TranscriptToolFamily::Task;
            let mut spec = test_spec(
                TranscriptBlockRole::Tool,
                TranscriptBlockContent::Tool {
                    family,
                    ids: if grouped {
                        vec!["tool-1".into(), "tool-2".into()]
                    } else {
                        vec!["tool".into()]
                    },
                    policy: TranscriptToolPolicy {
                        group_class: grouped.then_some(TranscriptToolGroupClass::Context),
                        member_count: if grouped { 2 } else { 1 },
                        visible_start: 0,
                        disclosure: TranscriptToolDisclosure::None,
                        status: TranscriptToolStatus::Succeeded,
                        motion: TranscriptBlockMotionDemand::None,
                        trailing_gap_cells: 0,
                    },
                    subagent: subagent.then_some(TranscriptSubagentPolicy {
                        mode: TranscriptSubagentMode::Foreground,
                        lifecycle: TranscriptSubagentLifecycle::Completed,
                        child_session_id: None,
                        output_truncated: false,
                        replay_read_only: false,
                    }),
                },
            );
            if grouped {
                spec.grouping.group_id = Some(TranscriptBlockId("group".into()));
                spec.grouping.member_count = 2;
            }
            spec
        }));

        // assert
        assert!(specs.iter().all(|spec| validate_block_spec(spec).is_ok()));
        assert!(specs.iter().any(|spec| {
            spec.role == TranscriptBlockRole::Tool
                && matches!(spec.content, TranscriptBlockContent::Tool { .. })
        }));
    }

    #[test]
    fn transcript_block_spec_resolves_compatibility_surface() {
        // arrange
        use super::super::ui_transcript_block_grammar::{
            resolve_block_surface, test_spec, TranscriptBlockContent, TranscriptBlockRole,
            TranscriptPromptState,
        };
        let spec = test_spec(
            TranscriptBlockRole::UserPrompt,
            TranscriptBlockContent::UserMessage {
                text: "source".into(),
                queued: false,
                wall_clock: None,
                state: TranscriptPromptState::Idle,
            },
        );
        let surface = TranscriptVisualEntryDraft {
            kind: TranscriptRenderSurfaceKind::User,
            leading_gap_rows: 0,
            trailing_gap_rows: 0,
            placement: TranscriptBlockPlacement::StickyPromptCandidate,
            show_outer_rail: false,
            rail_glyph: " ",
            rail_color: Color::Reset,
            surface: Color::Reset,
            lines: vec![Line::default()],
            interaction_rows: None,
            selection_rows: None,
            diff_hunk_offsets: Vec::new(),
            selected_rail: false,
            tool_rail_motion: None,
        };

        // act
        let resolved = resolve_block_surface(&spec, surface).expect("valid compatibility surface");

        // assert
        assert_eq!(resolved.kind, TranscriptRenderSurfaceKind::User);
    }

    #[test]
    fn transcript_block_spec_rejects_invalid_combinations() {
        // arrange
        use super::super::ui_transcript_block_grammar::*;
        let base = test_spec(
            TranscriptBlockRole::AssistantBody,
            TranscriptBlockContent::AssistantBody {
                text: String::new(),
                streaming: false,
                wall_clock: None,
                has_tools: false,
            },
        );
        let mut interaction = base.clone();
        interaction.interaction.selected = true;
        interaction.interaction.selectable = false;
        let mut motion = base.clone();
        motion.motion = TranscriptBlockMotionDemand::Active;
        motion.chrome.accent = false;
        let mut placement = base.clone();
        placement.placement = TranscriptBlockPlacement::PinnedFooter { outdent_cells: 0 };
        let mut disclosure = base.clone();
        disclosure.disclosure.expanded = true;
        let mut grouping = base;
        grouping.grouping.member_count = 2;

        // act
        let errors = [interaction, motion, placement, disclosure, grouping]
            .iter()
            .map(validate_block_spec)
            .collect::<Vec<_>>();

        // assert
        assert_eq!(
            errors,
            vec![
                Err(TranscriptGrammarError::InvalidInteraction),
                Err(TranscriptGrammarError::InvalidMotion),
                Err(TranscriptGrammarError::InvalidPlacement),
                Err(TranscriptGrammarError::InvalidDisclosure),
                Err(TranscriptGrammarError::InvalidGrouping),
            ]
        );
    }
}
