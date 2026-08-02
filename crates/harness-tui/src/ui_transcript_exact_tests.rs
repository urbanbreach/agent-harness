use super::*;

#[cfg(test)]
fn task_detail_blocks_text(blocks: &[TranscriptToolCallDetailBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            TranscriptToolCallDetailBlock::Message { text, .. } => Some(text.as_str()),
            TranscriptToolCallDetailBlock::Markdown { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[path = "ui_transcript_exact_tests/activity_flow.rs"]
mod activity_flow;
#[path = "ui_transcript_exact_tests/command_groups.rs"]
mod command_groups;
#[path = "ui_transcript_exact_tests/edit_diffs.rs"]
mod edit_diffs;
#[path = "ui_transcript_exact_tests/edit_folds.rs"]
mod edit_folds;
#[path = "ui_transcript_exact_tests/failure_shell.rs"]
mod failure_shell;
#[path = "ui_transcript_exact_tests/layout_permissions.rs"]
mod layout_permissions;
#[path = "ui_transcript_exact_tests/markdown_tables.rs"]
mod markdown_tables;
#[path = "ui_transcript_exact_tests/task_rows.rs"]
mod task_rows;
#[path = "ui_transcript_exact_tests/tool_identity.rs"]
mod tool_identity;

pub(crate) use activity_flow::*;
pub(crate) use edit_diffs::*;
pub(crate) use failure_shell::*;
pub(crate) use layout_permissions::*;
pub(crate) use markdown_tables::*;
pub(crate) use task_rows::*;
pub(crate) use tool_identity::*;
