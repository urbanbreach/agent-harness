use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TranscriptSectionRevisionKey {
    request_id: String,
    revision: u64,
    render_shape: TranscriptSectionRenderShapeKey,
    expansion_shape: TranscriptSectionExpansionShapeKey,
}

impl TranscriptSectionRevisionKey {
    pub(super) fn new(app: &AppState, section: &TranscriptTurnSection, revision: u64) -> Self {
        Self {
            request_id: section.request_id.clone(),
            revision,
            render_shape: TranscriptSectionRenderShapeKey::from_app(app),
            expansion_shape: TranscriptSectionExpansionShapeKey::from_app_section(app, section),
        }
    }

    pub(super) fn request_id(&self) -> &str {
        &self.request_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TranscriptSectionRenderShapeKey {
    thinking_visible: bool,
    timestamps_visible: bool,
    show_tool_details: bool,
    show_generic_tool_output: bool,
    stacked_diffs: bool,
}

impl TranscriptSectionRenderShapeKey {
    fn from_app(app: &AppState) -> Self {
        Self {
            thinking_visible: app.transcript_thinking_visible(),
            timestamps_visible: app.transcript_timestamps_visible(),
            show_tool_details: app.tool_details_visible(),
            show_generic_tool_output: app.generic_tool_output_visible(),
            stacked_diffs: app.transcript_view.stacked_transcript_diffs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TranscriptSectionExpansionShapeKey {
    expanded_tool_outputs: Vec<String>,
    expanded_patch_file_outputs: Vec<String>,
}

impl TranscriptSectionExpansionShapeKey {
    fn from_app_section(app: &AppState, section: &TranscriptTurnSection) -> Self {
        let tool_call_ids = section
            .tool_calls
            .iter()
            .map(|tool_call| tool_call.tool_call_id.as_str())
            .collect::<Vec<_>>();
        let expanded_tool_outputs = section
            .tool_calls
            .iter()
            .filter(|tool_call| {
                app.transcript_view
                    .expanded_tool_outputs
                    .contains(&tool_call.tool_call_id)
            })
            .map(|tool_call| tool_call.tool_call_id.clone())
            .collect();
        let expanded_patch_file_outputs = app
            .transcript_view
            .expanded_patch_file_outputs
            .iter()
            .filter(|file_key| {
                file_key
                    .split_once('\u{1f}')
                    .is_some_and(|(tool_call_id, _)| tool_call_ids.contains(&tool_call_id))
            })
            .cloned()
            .collect();

        Self {
            expanded_tool_outputs,
            expanded_patch_file_outputs,
        }
    }
}
