use std::borrow::Cow;

use super::streaming_text::StreamingText;
use super::*;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum StreamKey {
    Text(String),
    Reasoning(String),
    Arguments(String),
    Output(String),
    Command(String),
}

impl AppState {
    pub(super) fn observe_streaming_text(&mut self, event: &LiveEventEnvelope) {
        if self.reduced_motion || !self.presentation_is_live() {
            return;
        }
        let now = self.now();
        let activities = &self.projection.activities;
        let (key, text, initial) = match &event.payload {
            LiveEventV1::ProviderTextDelta { request_id, delta }
            | LiveEventV1::ProviderReasoningDelta { request_id, delta } => {
                let Some(activity) = activities.iter().find(|activity| {
                    event.correlation_id.as_deref() == Some(&activity.request_id)
                        || activity.request_id == request_id.as_str()
                        || activity.request_data.as_ref().is_some_and(|request| {
                            request.request_id.as_str() == request_id.as_str()
                        })
                }) else {
                    return;
                };
                let (key, text) = if matches!(event.payload, LiveEventV1::ProviderTextDelta { .. })
                {
                    (
                        StreamKey::Text(activity.request_id.clone()),
                        &activity.transcript_text,
                    )
                } else {
                    (
                        StreamKey::Reasoning(activity.request_id.clone()),
                        &activity.thinking_text,
                    )
                };
                (key, text.as_str(), text.len().saturating_sub(delta.len()))
            }
            LiveEventV1::ProviderToolInputDelta {
                tool_call_id,
                delta,
                ..
            } => {
                let Some(tool) = activities
                    .iter()
                    .flat_map(|activity| &activity.tool_calls)
                    .find(|tool| tool.tool_call_id == tool_call_id.as_str())
                else {
                    return;
                };
                (
                    StreamKey::Arguments(tool.tool_call_id.clone()),
                    tool.args_summary.as_str(),
                    tool.args_summary.len().saturating_sub(delta.len()),
                )
            }
            LiveEventV1::EvalProgress { tool_call_id, .. } => {
                let Some((output, details)) = self.projection.live_evals.get(tool_call_id.as_str())
                else {
                    return;
                };
                (
                    StreamKey::Output(tool_call_id.to_string()),
                    details
                        .pointer("/cells/0/output")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(output),
                    0,
                )
            }
            _ => return,
        };
        // Redact the complete received text before a display prefix can expose part of a secret.
        let initial = crate::ui::safe_product_text(text.get(..initial).unwrap_or_default()).len();
        let text = crate::ui::safe_product_text(text);
        self.streaming_text
            .entry(key)
            .and_modify(|stream| stream.update(&text, now))
            .or_insert_with(|| StreamingText::new(&text, initial.min(text.len()), now));
    }

    pub(crate) fn smoothed_activity<'a>(
        &self,
        activity: &'a ActivityEntry,
    ) -> Cow<'a, ActivityEntry> {
        let mut visible = Cow::Borrowed(activity);
        if let Some(text) = self.stream_prefix(&StreamKey::Text(activity.request_id.clone())) {
            visible.to_mut().transcript_text = text.to_owned();
        }
        if let Some(text) = self.stream_prefix(&StreamKey::Reasoning(activity.request_id.clone())) {
            visible.to_mut().thinking_text = text.to_owned();
        }
        visible
    }

    pub(crate) fn smoothed_tool<'a>(&self, tool: &'a ToolCallEntry) -> Cow<'a, ToolCallEntry> {
        let mut visible = Cow::Borrowed(tool);
        if let Some(text) = self.stream_prefix(&StreamKey::Arguments(tool.tool_call_id.clone())) {
            visible.to_mut().args_summary = text.to_owned();
        }
        if let Some(text) = self.stream_prefix(&StreamKey::Output(tool.tool_call_id.clone())) {
            let tool = visible.to_mut();
            tool.output_summary = Some(text.to_owned());
            if let Some(output) = tool
                .output_json
                .as_mut()
                .and_then(|data| data.pointer_mut("/cells/0/output"))
            {
                *output = text.into();
            }
        }
        visible
    }

    fn stream_prefix(&self, key: &StreamKey) -> Option<&str> {
        self.streaming_text
            .get(key)
            .filter(|stream| stream.pending())
            .map(StreamingText::visible)
    }

    pub(super) fn advance_streaming_text(&mut self) {
        let now = self.now();
        let mut changed = false;
        let mut dirty_from = usize::MAX;
        for (key, stream) in &mut self.streaming_text {
            if !stream.advance(now) {
                continue;
            }
            changed = true;
            let index = self
                .projection
                .activities
                .iter()
                .position(|activity| match key {
                    StreamKey::Text(id) | StreamKey::Reasoning(id) => activity.request_id == *id,
                    StreamKey::Arguments(id) | StreamKey::Output(id) => activity
                        .tool_calls
                        .iter()
                        .any(|tool| tool.tool_call_id == *id),
                    StreamKey::Command(_) => false,
                });
            dirty_from = dirty_from.min(index.unwrap_or(usize::MAX));
        }
        if changed {
            self.transcript_view.prepared.invalidate_from(dirty_from);
            self.refresh_transcript_viewer();
            self.refresh_streaming_command();
        }
    }

    pub(super) fn settle_streaming_text(&mut self) {
        if self.streaming_text.is_empty() {
            return;
        }
        for stream in self.streaming_text.values_mut() {
            stream.finish();
        }
        self.bump_transcript_render_epoch();
        self.refresh_transcript_viewer();
        self.refresh_streaming_command();
        self.streaming_text.clear();
    }

    pub(super) fn refresh_streaming_command(&mut self) {
        if let Some(id) = &self.inspected_command
            && let Some(stream) = self.streaming_text.get(&StreamKey::Command(id.clone()))
            && let Some(viewer) = &mut self.transcript_viewer
        {
            let text = stream.visible();
            let _ = viewer.update_content(crate::transcript_block_viewer::ViewerBlockContent::new(
                text,
                Some(text),
            ));
        }
    }
}
