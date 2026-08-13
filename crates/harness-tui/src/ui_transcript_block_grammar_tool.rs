use super::*;

pub(super) fn group_policy(summary: &TranscriptToolGroupSummary) -> TranscriptToolPolicy {
    TranscriptToolPolicy {
        group_class: Some(match summary.kind {
            TranscriptToolGroupKind::Commands => TranscriptToolGroupClass::Commands,
            TranscriptToolGroupKind::Context => TranscriptToolGroupClass::Context,
        }),
        member_count: summary.member_count,
        visible_start: if summary.kind == TranscriptToolGroupKind::Commands
            && summary.disclosure != TranscriptToolDisclosureMode::Expanded
        {
            summary.member_count.saturating_sub(10)
        } else {
            0
        },
        disclosure: match summary.disclosure {
            TranscriptToolDisclosureMode::Collapsed => TranscriptToolDisclosure::Collapsed,
            TranscriptToolDisclosureMode::Preview => TranscriptToolDisclosure::Preview,
            TranscriptToolDisclosureMode::Expanded => TranscriptToolDisclosure::Expanded,
        },
        status: group_status(summary),
        motion: if summary.running_count > 0 || summary.queued_count > 0 {
            TranscriptBlockMotionDemand::Active
        } else {
            TranscriptBlockMotionDemand::None
        },
        trailing_gap_cells: 0,
    }
}

pub(super) fn apply_tool_policy(spec: &mut TranscriptBlockSpec) {
    let TranscriptBlockContent::Tool { family, policy, .. } = spec.content else {
        return;
    };
    let active = matches!(
        policy.status,
        TranscriptToolStatus::Queued | TranscriptToolStatus::Running
    );
    spec.chrome = TranscriptBlockChrome {
        accent: active
            || policy.motion != TranscriptBlockMotionDemand::None
            || matches!(policy.status, TranscriptToolStatus::Failed),
        rail: matches!(
            family,
            TranscriptToolFamily::Shell | TranscriptToolFamily::Diff
        ),
    };
    spec.interaction = TranscriptBlockInteraction {
        selectable: false,
        selected: false,
        hoverable: true,
        focusable: true,
    };
    spec.disclosure = TranscriptBlockDisclosure {
        available: policy.disclosure != TranscriptToolDisclosure::None,
        expanded: policy.disclosure == TranscriptToolDisclosure::Expanded,
    };
    spec.compact = TranscriptBlockCompactPolicy::ElideDetails;
    spec.motion = policy.motion;
}

pub(super) fn tool_family(tool: &TranscriptToolCallSection) -> TranscriptToolFamily {
    if tool
        .detail_blocks
        .iter()
        .any(|block| matches!(block, TranscriptToolCallDetailBlock::StructuredDiff { .. }))
    {
        return TranscriptToolFamily::Diff;
    }
    match tool.header.tool_id.as_str() {
        "question" => TranscriptToolFamily::Question,
        "bash" | "shell.run" => TranscriptToolFamily::Shell,
        _ if tool.header.presentation.status == ToolCallPresentationStatus::Waiting => {
            TranscriptToolFamily::Permission
        }
        "apply_patch" | "edit" | "write" | "fs.write" => TranscriptToolFamily::Diff,
        "task" | "agent.spawn" => TranscriptToolFamily::Subagent,
        _ => TranscriptToolFamily::Generic,
    }
}

pub(super) fn tool_policy(tool: &TranscriptToolCallSection) -> TranscriptToolPolicy {
    TranscriptToolPolicy {
        group_class: None,
        member_count: 1,
        visible_start: 0,
        disclosure: match tool.header.disclosure_state {
            None => TranscriptToolDisclosure::None,
            Some(TranscriptToolCallDisclosureState::Collapsed) if tool.details_preview_visible => {
                TranscriptToolDisclosure::Preview
            }
            Some(TranscriptToolCallDisclosureState::Collapsed) => {
                TranscriptToolDisclosure::Collapsed
            }
            Some(TranscriptToolCallDisclosureState::Expanded) => TranscriptToolDisclosure::Expanded,
        },
        status: tool_status(tool.header.presentation.status),
        motion: match tool.rail_motion {
            ToolRailMotion::Running { .. } => TranscriptBlockMotionDemand::Active,
            ToolRailMotion::FinishFlash { .. } => TranscriptBlockMotionDemand::Finish,
            ToolRailMotion::Waiting | ToolRailMotion::Queued | ToolRailMotion::Settled => {
                TranscriptBlockMotionDemand::None
            }
        },
        trailing_gap_cells: 0,
    }
}

pub(super) fn subagent_policy(
    tool: &TranscriptToolCallSection,
) -> Option<TranscriptSubagentPolicy> {
    (tool_family(tool) == TranscriptToolFamily::Subagent).then(|| TranscriptSubagentPolicy {
        mode: if tool.subagent_background {
            TranscriptSubagentMode::Background
        } else {
            TranscriptSubagentMode::Foreground
        },
        lifecycle: match tool.header.presentation.status {
            ToolCallPresentationStatus::Queued => TranscriptSubagentLifecycle::Queued,
            ToolCallPresentationStatus::Running | ToolCallPresentationStatus::Waiting => {
                TranscriptSubagentLifecycle::Running
            }
            ToolCallPresentationStatus::Succeeded => TranscriptSubagentLifecycle::Completed,
            ToolCallPresentationStatus::Failed => TranscriptSubagentLifecycle::Failed,
            ToolCallPresentationStatus::Cancelled => TranscriptSubagentLifecycle::Cancelled,
        },
        child_session_id: tool.child_session_id.clone(),
        output_truncated: tool.output_truncated,
        replay_read_only: tool.replay_read_only,
    })
}

const fn tool_status(status: ToolCallPresentationStatus) -> TranscriptToolStatus {
    match status {
        ToolCallPresentationStatus::Queued => TranscriptToolStatus::Queued,
        ToolCallPresentationStatus::Running => TranscriptToolStatus::Running,
        ToolCallPresentationStatus::Waiting => TranscriptToolStatus::Waiting,
        ToolCallPresentationStatus::Succeeded => TranscriptToolStatus::Succeeded,
        ToolCallPresentationStatus::Failed => TranscriptToolStatus::Failed,
        ToolCallPresentationStatus::Cancelled => TranscriptToolStatus::Cancelled,
    }
}

fn group_status(summary: &TranscriptToolGroupSummary) -> TranscriptToolStatus {
    if summary.waiting_count > 0 {
        TranscriptToolStatus::Waiting
    } else if summary.running_count > 0 {
        TranscriptToolStatus::Running
    } else if summary.queued_count > 0 {
        TranscriptToolStatus::Queued
    } else if summary.failed_count > 0 {
        TranscriptToolStatus::Failed
    } else if summary.cancelled_count > 0 {
        TranscriptToolStatus::Cancelled
    } else {
        TranscriptToolStatus::Succeeded
    }
}
