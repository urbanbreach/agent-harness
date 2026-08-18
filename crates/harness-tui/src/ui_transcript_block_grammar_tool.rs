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
            TranscriptToolDisclosureMode::Preview
                if summary.kind == TranscriptToolGroupKind::Commands =>
            {
                TranscriptToolDisclosure::Preview
            }
            TranscriptToolDisclosureMode::Preview => TranscriptToolDisclosure::Collapsed,
            TranscriptToolDisclosureMode::Expanded => TranscriptToolDisclosure::Expanded,
        },
        status: group_status(summary),
        motion: if summary.running_count > 0 {
            TranscriptBlockMotionDemand::Active
        } else {
            TranscriptBlockMotionDemand::None
        },
        trailing_gap_cells: 0,
    }
}

pub(super) fn apply_tool_policy(spec: &mut TranscriptBlockSpec) {
    let TranscriptBlockContent::Tool { policy, .. } = spec.content else {
        return;
    };
    let active = policy.status == TranscriptToolStatus::Running;
    spec.chrome = TranscriptBlockChrome {
        accent: active || policy.motion != TranscriptBlockMotionDemand::None,
        rail: false,
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
        "bash" | "shell.run" => TranscriptToolFamily::Execute,
        "apply_patch"
        | "edit"
        | "write"
        | "fs.write"
        | "edit.hashline_apply"
        | "ast_grep_replace"
        | "lsp.rename" => TranscriptToolFamily::Edit,
        "task" | "agent.spawn" | "background_output" | "background_cancel" => {
            TranscriptToolFamily::Task
        }
        "fs.read" | "read" | "session_read" => TranscriptToolFamily::Read,
        "fs.glob" | "glob" | "fs.grep" | "grep" | "search.code" | "session_search"
        | "ast_grep_search" => TranscriptToolFamily::Search,
        "fs.ls" | "list" | "session_list" => TranscriptToolFamily::List,
        "web.fetch" | "search.web" => TranscriptToolFamily::Web,
        _ if tool.header.presentation.status == ToolCallPresentationStatus::Waiting => {
            TranscriptToolFamily::Permission
        }
        _ => TranscriptToolFamily::Unknown,
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
            ToolRailMotion::Running { .. } => match tool.header.presentation.status {
                ToolCallPresentationStatus::Running => TranscriptBlockMotionDemand::Active,
                ToolCallPresentationStatus::Queued
                | ToolCallPresentationStatus::Waiting
                | ToolCallPresentationStatus::Succeeded
                | ToolCallPresentationStatus::Failed
                | ToolCallPresentationStatus::Cancelled => TranscriptBlockMotionDemand::None,
            },
            ToolRailMotion::FinishFlash { .. }
            | ToolRailMotion::Waiting
            | ToolRailMotion::Queued
            | ToolRailMotion::Settled => TranscriptBlockMotionDemand::None,
        },
        trailing_gap_cells: 0,
    }
}

pub(super) fn subagent_policy(
    tool: &TranscriptToolCallSection,
) -> Option<TranscriptSubagentPolicy> {
    (tool_family(tool) == TranscriptToolFamily::Task).then(|| TranscriptSubagentPolicy {
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
