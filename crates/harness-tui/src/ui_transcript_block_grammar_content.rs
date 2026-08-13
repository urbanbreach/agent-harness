use super::*;

pub(super) fn content_for_part(
    turn: &TranscriptTurnSection,
    part_index: usize,
    part: &TranscriptAssistantPart,
) -> (TranscriptBlockRole, TranscriptBlockContent) {
    match part {
        TranscriptAssistantPart::Reasoning(value) => (
            TranscriptBlockRole::Reasoning,
            TranscriptBlockContent::Reasoning {
                text: value.text.clone(),
                active: turn.header.status == ActivityStatus::Streaming
                    && part_index + 1 == turn.assistant_parts.len()
                    && !reasoning_force_completed(turn),
                expanded: turn.reasoning_expanded,
                duration_ms: turn.header.thinking_duration_ms,
                motion_enabled: transcript_motion_enabled(),
            },
        ),
        TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text)) => (
            TranscriptBlockRole::AssistantBody,
            TranscriptBlockContent::AssistantBody {
                text: text.clone(),
                streaming: false,
                wall_clock: turn.footer_timestamp.clone(),
                has_tools: turn
                    .assistant_parts
                    .iter()
                    .any(|part| matches!(part, TranscriptAssistantPart::ToolCall(_))),
            },
        ),
        TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text)) => (
            TranscriptBlockRole::AssistantBody,
            TranscriptBlockContent::AssistantBody {
                text: text.clone(),
                streaming: true,
                wall_clock: turn.footer_timestamp.clone(),
                has_tools: turn
                    .assistant_parts
                    .iter()
                    .any(|part| matches!(part, TranscriptAssistantPart::ToolCall(_))),
            },
        ),
        TranscriptAssistantPart::ToolCall(tool) => (
            TranscriptBlockRole::Tool,
            TranscriptBlockContent::Tool {
                family: super::tool::tool_family(tool),
                ids: std::iter::once(tool.tool_call_id.clone())
                    .chain(tool.coalesced_tool_call_ids.iter().cloned())
                    .collect(),
                policy: super::tool::tool_policy(tool),
                subagent: super::tool::subagent_policy(tool),
            },
        ),
        TranscriptAssistantPart::Error(error) => (
            TranscriptBlockRole::Error,
            TranscriptBlockContent::Error {
                message: error.text.clone(),
            },
        ),
        TranscriptAssistantPart::Compaction(compaction) => (
            TranscriptBlockRole::Compaction,
            TranscriptBlockContent::Compaction {
                branch_summary: compaction.kind == TranscriptCompactionKind::BranchSummary,
                summary: compaction.summary.clone(),
                tokens_before: compaction.tokens_before,
                read_files: compaction.read_files.clone(),
                modified_files: compaction.modified_files.clone(),
            },
        ),
    }
}

pub(super) fn apply_reasoning_policy(spec: &mut TranscriptBlockSpec) {
    let TranscriptBlockContent::Reasoning {
        active, expanded, ..
    } = spec.content
    else {
        return;
    };
    spec.chrome.accent = active;
    spec.fold = TranscriptBlockFold {
        foldable: true,
        expanded,
    };
    spec.interaction = TranscriptBlockInteraction {
        selectable: false,
        selected: false,
        hoverable: true,
        focusable: true,
    };
    spec.disclosure = TranscriptBlockDisclosure {
        available: true,
        expanded,
    };
    spec.compact = TranscriptBlockCompactPolicy::Collapse;
    spec.motion = if active && transcript_motion_enabled() {
        TranscriptBlockMotionDemand::Active
    } else {
        TranscriptBlockMotionDemand::None
    };
}

pub(super) fn footer_lifecycle(turn: &TranscriptTurnSection) -> TranscriptFooterLifecycle {
    if turn.header.retry.as_ref().is_some_and(|retry| retry.attempt > 0) {
        TranscriptFooterLifecycle::Retry
    } else if turn.header.status == ActivityStatus::Streaming {
        TranscriptFooterLifecycle::Responding
    } else if turn.assistant_parts.iter().any(|part| {
        matches!(part, TranscriptAssistantPart::ToolCall(tool) if tool.header.tool_id == "question")
    }) {
        TranscriptFooterLifecycle::Question
    } else if reasoning_force_completed(turn) {
        TranscriptFooterLifecycle::Permission
    } else {
        TranscriptFooterLifecycle::Settled
    }
}

pub(super) fn footer_content(
    turn: &TranscriptTurnSection,
    lifecycle: TranscriptFooterLifecycle,
) -> TranscriptFooterContent {
    let metadata = TranscriptFooterMetadata {
        duration_ms: turn.header.duration_ms,
        total_tokens: turn.header.total_tokens,
    };
    match lifecycle {
        TranscriptFooterLifecycle::Permission => {
            let tool = turn.assistant_parts.iter().find_map(|part| match part {
                TranscriptAssistantPart::ToolCall(tool)
                    if tool.header.presentation.status == ToolCallPresentationStatus::Waiting
                        && tool.header.tool_id != "question" =>
                {
                    Some(tool.as_ref())
                }
                _ => None,
            });
            let tool_id = tool.map_or_else(|| "tool".into(), |tool| tool.header.tool_id.clone());
            let label = tool.map_or_else(
                || "Run tool".into(),
                |tool| format!("Run {}", tool.header.title),
            );
            TranscriptFooterContent::Permission {
                tool_id,
                label,
                metadata,
            }
        }
        TranscriptFooterLifecycle::Question => {
            let label = turn
                .assistant_parts
                .iter()
                .find_map(|part| match part {
                    TranscriptAssistantPart::ToolCall(tool)
                        if tool.header.tool_id == "question" =>
                    {
                        Some(tool.header.title.trim())
                    }
                    _ => None,
                })
                .filter(|label| !label.is_empty())
                .map_or_else(
                    || "Waiting on answers".into(),
                    |label| format!("Waiting on answers for {label}"),
                );
            TranscriptFooterContent::Question { label, metadata }
        }
        TranscriptFooterLifecycle::Retry => TranscriptFooterContent::Retry {
            attempt: turn.header.retry.map_or(0, |retry| retry.attempt),
            metadata,
        },
        TranscriptFooterLifecycle::Responding => TranscriptFooterContent::Responding { metadata },
        TranscriptFooterLifecycle::Settled => TranscriptFooterContent::Settled,
    }
}

pub(super) fn lifecycle_state(turn: &TranscriptTurnSection) -> TranscriptLifecycleState {
    match turn.header.status {
        ActivityStatus::Queued => TranscriptLifecycleState::Queued,
        ActivityStatus::Streaming => turn.header.retry.filter(|retry| retry.attempt > 0).map_or(
            TranscriptLifecycleState::Responding,
            |retry| TranscriptLifecycleState::Retrying {
                attempt: retry.attempt,
                max_attempts: retry.max_attempts,
                elapsed_ms: turn.header.retry_elapsed_ms,
            },
        ),
        ActivityStatus::Error => {
            if turn.assistant_parts.iter().any(|part| {
                matches!(part, TranscriptAssistantPart::Error(error) if is_cancelled_error(&error.text))
            }) {
                TranscriptLifecycleState::Cancelled
            } else {
                TranscriptLifecycleState::Failed
            }
        }
        ActivityStatus::Done if turn.header.retry.is_some() => TranscriptLifecycleState::Recovered,
        ActivityStatus::Done => TranscriptLifecycleState::Completed,
    }
}

fn is_cancelled_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    ["interrupted", "cancelled", "canceled", "user cancel"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn reasoning_force_completed(turn: &TranscriptTurnSection) -> bool {
    turn.assistant_parts.iter().any(|part| {
        matches!(part, TranscriptAssistantPart::ToolCall(tool) if tool.header.tool_id == "question" || tool.header.presentation.status == ToolCallPresentationStatus::Waiting)
    })
}

fn transcript_motion_enabled() -> bool {
    std::env::var_os("HARNESS_DISABLE_ANIMATIONS").is_none()
        && !std::env::var("HARNESS_TUI_REDUCED_MOTION")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
}
