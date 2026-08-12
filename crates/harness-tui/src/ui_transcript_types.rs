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
}

impl TranscriptToolVerb {
    pub(super) fn from_tool_id(tool_id: &str) -> Option<Self> {
        match tool_id {
            "shell.run" | "bash" => Some(Self::Run),
            "fs.read" | "read" => Some(Self::Read),
            "fs.glob" | "glob" | "fs.grep" | "grep" => Some(Self::Search),
            "fs.ls" | "list" => Some(Self::List),
            "skill" | "skill.load" => Some(Self::Skill),
            _ => None,
        }
    }

    pub(super) const fn group_kind(self) -> TranscriptToolGroupKind {
        match self {
            Self::Run => TranscriptToolGroupKind::Commands,
            Self::Read | Self::Search | Self::List | Self::Skill => {
                TranscriptToolGroupKind::Context
            }
        }
    }
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
        let first_verb = TranscriptToolVerb::from_tool_id(&first.header.tool_id)?;
        let kind = first_verb.group_kind();
        let mut summary = Self {
            kind,
            span_len: 0,
            member_count: 0,
            verbs: Vec::new(),
            queued_count: 0,
            running_count: 0,
            waiting_count: 0,
            succeeded_count: 0,
            failed_count: 0,
            cancelled_count: 0,
            disclosure: TranscriptToolDisclosureMode::Collapsed,
            duration_ms: None,
            result_count: None,
        };

        for part in parts {
            if matches!(part, TranscriptAssistantPart::Reasoning(_)) && summary.member_count > 0 {
                summary.span_len = summary.span_len.saturating_add(1);
                continue;
            }
            let Some(tool_call) = part.tool_call() else {
                break;
            };
            let Some(verb) = TranscriptToolVerb::from_tool_id(&tool_call.header.tool_id) else {
                break;
            };
            if verb.group_kind() != kind {
                break;
            }

            summary.span_len = summary.span_len.saturating_add(1);
            summary.member_count += 1;
            if !summary.verbs.contains(&verb) {
                summary.verbs.push(verb);
            }
            match tool_call.header.presentation.status {
                ToolCallPresentationStatus::Queued => summary.queued_count += 1,
                ToolCallPresentationStatus::Running => summary.running_count += 1,
                ToolCallPresentationStatus::Waiting => summary.waiting_count += 1,
                ToolCallPresentationStatus::Succeeded => summary.succeeded_count += 1,
                ToolCallPresentationStatus::Failed => summary.failed_count += 1,
                ToolCallPresentationStatus::Cancelled => summary.cancelled_count += 1,
            }
            summary.disclosure = if tool_call.expanded {
                TranscriptToolDisclosureMode::Expanded
            } else if summary.disclosure != TranscriptToolDisclosureMode::Expanded
                && tool_call.details_preview_visible
            {
                TranscriptToolDisclosureMode::Preview
            } else {
                summary.disclosure
            };
            if let Some(duration_ms) = tool_call.header.presentation.duration_ms {
                summary.duration_ms = Some(
                    summary
                        .duration_ms
                        .unwrap_or_default()
                        .saturating_add(duration_ms),
                );
            }
            if let Some(result_count) = tool_call.header.presentation.result_count {
                summary.result_count = Some(
                    summary
                        .result_count
                        .unwrap_or_default()
                        .saturating_add(result_count),
                );
            }
        }

        (summary.member_count > 0).then_some(summary)
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
    pub(super) activity_first_seq: u64,
    pub(super) request_id: String,
    pub(super) user_message: Option<TranscriptUserMessageSection>,
    pub(super) show_footer: bool,
    pub(super) footer_timestamp: Option<String>,
    pub(super) animation_phase: usize,
    pub(super) reasoning_expanded: bool,
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

        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("command group");

        assert_eq!(summary.kind, TranscriptToolGroupKind::Commands);
        assert_eq!(summary.member_count, 3);
        assert_eq!(summary.succeeded_count, 1);
        assert_eq!(summary.running_count, 1);
        assert_eq!(summary.failed_count, 1);
    }

    #[test]
    fn adjacent_group_stops_at_typed_group_boundary() {
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

        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        assert_eq!(summary.kind, TranscriptToolGroupKind::Context);
        assert_eq!(summary.member_count, 2);
        assert_eq!(
            summary.verbs,
            vec![TranscriptToolVerb::Read, TranscriptToolVerb::Search]
        );
    }

    #[test]
    fn group_disclosure_promotes_collapsed_to_preview_then_expanded() {
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
    fn finish_acknowledgement_duration_matches_design_contract() {
        assert_eq!(crate::app::TOOL_FINISH_FLASH_DURATION.as_millis(), 400);
    }

    #[test]
    fn completed_reasoning_is_transparent_inside_a_tool_group_span() {
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

        let summary = TranscriptToolGroupSummary::from_adjacent(&parts).expect("context group");

        assert_eq!(summary.member_count, 2);
        assert_eq!(summary.span_len, 3);
    }
}
