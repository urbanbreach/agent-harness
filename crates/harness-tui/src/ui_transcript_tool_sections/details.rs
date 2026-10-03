use super::*;

const OUTPUT_LINE_CLAMP: usize = 3;

pub(super) fn finish(
    row: &mut TranscriptToolCallSection,
    tool: &ToolCallEntry,
    app: &AppState,
    generic_visible: bool,
    visible: bool,
) {
    let id = tool.effective_tool_id();
    let blocks = &mut row.detail_blocks;
    suppress_unexecuted_edit_proposals(blocks, tool);
    diffs::attach_recorded_diff_sources(blocks, tool, app);
    set_diff_highlight_phase(blocks, tool.status == ToolCallDisplayStatus::Succeeded);
    if tool.status == ToolCallDisplayStatus::Succeeded
        && matches!(id, "edit.hashline_apply" | "edit" | "write" | "fs.write")
    {
        row.header.title = edit_tool_action(tool).to_owned();
    }
    if visible {
        if let Some(output) = crate::ui::ui_recorded_tool_output::project(tool) {
            blocks.clear();
            blocks.push(TranscriptToolCallDetailBlock::Recorded(output));
        }
    }
    if push_edit_diagnostics(blocks, tool, row.expanded) {
        row.header.visual_style = TranscriptToolCallVisualStyle::Block;
    }
    if blocks.is_empty() && generic_visible {
        let error = (tool.status == ToolCallDisplayStatus::Failed)
            .then(|| tool_error_text(tool))
            .flatten();
        if let Some(output) = error.as_deref().or(tool.output_summary.as_deref()) {
            push_collapsible_output_block(
                blocks,
                output,
                if tool.status == ToolCallDisplayStatus::Failed {
                    TranscriptToolCallDetailTone::Error
                } else {
                    TranscriptToolCallDetailTone::Primary
                },
                OUTPUT_LINE_CLAMP,
                row.expanded,
            );
        }
    }
    if !matches!(
        id,
        "user.question" | "question" | "spawn_subagent" | "agent.spawn" | "task"
    ) {
        push_failed_tool_error_block(blocks, tool);
    }
    push_truncated_output_artifact_block(blocks, tool);
    prepare_failed_tool_details(blocks, tool);
}

fn suppress_unexecuted_edit_proposals(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    // Input-derived replacements are proposals, not output from a completed
    // edit. Keep recorded diff artifacts, including partial failure results.
    if matches!(tool_call.effective_tool_id(), "write" | "fs.write" | "edit")
        && tool_call.status != ToolCallDisplayStatus::Succeeded
        && tool_call_diff_artifacts(tool_call).is_empty()
    {
        detail_blocks
            .retain(|block| !matches!(block, TranscriptToolCallDetailBlock::StructuredDiff { .. }));
    }
}

fn prepare_failed_tool_details(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    if tool_error_hidden_inline(tool_call) {
        detail_blocks.clear();
        return;
    }

    // Commands and edits separate errors from their headers. Edits use Grok's
    // muted decoration; command failures retain the error foreground.
    if tool_call.status == ToolCallDisplayStatus::Failed
        && matches!(
            tool_call.effective_tool_id(),
            "shell.run" | "bash" | "edit" | "write" | "fs.write" | "edit.hashline_apply"
        )
    {
        let error_tone = if matches!(tool_call.effective_tool_id(), "shell.run" | "bash") {
            TranscriptToolCallDetailTone::Error
        } else {
            TranscriptToolCallDetailTone::Secondary
        };
        for block in detail_blocks {
            if let TranscriptToolCallDetailBlock::Message { text, tone } = block {
                if *tone == TranscriptToolCallDetailTone::Error {
                    *text = format!("\n{text}");
                    *tone = error_tone;
                }
            }
        }
    }
}

fn push_truncated_output_artifact_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool_call: &crate::app::ToolCallEntry,
) {
    if tool_call.truncated_output.is_none() || tool_call.artifact_refs.is_empty() {
        return;
    }
    if !detail_blocks.iter().any(|block| {
        matches!(
            block,
            TranscriptToolCallDetailBlock::Message {
                tone: TranscriptToolCallDetailTone::Primary,
                ..
            } | TranscriptToolCallDetailBlock::BashPanel { .. }
        )
    }) {
        return;
    }
    let artifact = &tool_call.artifact_refs[0];
    let text = format!(
        "Output truncated · full output at {artifact}",
        artifact = artifact.path
    );
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text,
        tone: TranscriptToolCallDetailTone::Secondary,
    });
}

fn push_edit_diagnostics(
    blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    tool: &crate::app::ToolCallEntry,
    expanded: bool,
) -> bool {
    if tool.status != ToolCallDisplayStatus::Succeeded
        || !matches!(
            tool.effective_tool_id(),
            "edit.hashline_apply" | "edit" | "write" | "fs.write" | "apply_patch"
        )
    {
        return false;
    }
    let Some((_, diagnostics)) = tool
        .output_summary
        .as_deref()
        .and_then(|text| text.split_once("\n\nLSP "))
    else {
        return false;
    };
    // Edit diffs replace the ordinary output body; keep their verification result alongside it.
    push_collapsible_output_block(
        blocks,
        &format!("LSP {diagnostics}"),
        TranscriptToolCallDetailTone::Secondary,
        OUTPUT_LINE_CLAMP,
        expanded,
    );
    true
}

fn push_collapsible_output_block(
    detail_blocks: &mut Vec<TranscriptToolCallDetailBlock>,
    output: &str,
    tone: TranscriptToolCallDetailTone,
    max_lines: usize,
    expanded: bool,
) {
    let preview = collapsible_output_preview(output, max_lines, expanded);
    detail_blocks.push(TranscriptToolCallDetailBlock::Message {
        text: if tone == TranscriptToolCallDetailTone::Primary {
            format!("\n{preview}")
        } else {
            preview
        },
        tone: if tone == TranscriptToolCallDetailTone::Primary {
            TranscriptToolCallDetailTone::Secondary
        } else {
            tone
        },
    });
}
