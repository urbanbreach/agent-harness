use std::collections::BTreeMap;

use harness_core::event::EventV1;
use harness_core::session::{AssistantPart, CanonicalProviderFragmentKind};

use super::ui_transcript_sections::events_for_activity;
use super::*;

type SequencedPart = (u64, TranscriptAssistantPart);

pub(super) fn build_assistant_parts(
    app: &AppState,
    activity: &ActivityEntry,
    tools: Vec<TranscriptOrderedToolCallSection>,
) -> (
    Vec<TranscriptAssistantPart>,
    Vec<TranscriptAssistantPartSourceId>,
) {
    let thinking = app.transcript_thinking_visible();
    let mut events = events_for_activity(app, activity)
        .filter(|event| turn_event_matches_activity(event, &activity.request_id))
        .peekable();
    let mut parts = if events.peek().is_none() {
        fallback(app, activity, tools)
    } else {
        let committed_tools = events
            .clone()
            .filter_map(|event| match &event.payload {
                EventV1::AssistantMessageFinished(data) => Some(&data.parts),
                _ => None,
            })
            .flatten()
            .filter_map(|part| match part {
                AssistantPart::ToolCall(tool) => Some(tool.tool_call_id.as_str()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut builder = AssistantParts {
            parts: Vec::new(),
            tools: tools
                .into_iter()
                .map(|tool| (tool.tool_call_id, (tool.first_seq, tool.section)))
                .collect(),
            pending_text: None,
            saw_reasoning: false,
            saw_body: false,
            saw_tool: false,
            thinking,
            activity,
        };
        for event in events {
            if let Some(fragment) =
                harness_core::session::canonical_provider_fragment_for_event(event)
            {
                match fragment.kind {
                    CanonicalProviderFragmentKind::Reasoning if thinking => {
                        builder.saw_reasoning = true;
                        builder.settle_last();
                        builder.text(event.seq, true, fragment.delta);
                    }
                    CanonicalProviderFragmentKind::Text if builder.saw_tool => {
                        builder.saw_body = true;
                        builder.text(event.seq, false, fragment.delta);
                    }
                    CanonicalProviderFragmentKind::Text => {
                        builder
                            .pending_text
                            .get_or_insert_with(|| (event.seq, String::new()))
                            .1
                            .push_str(fragment.delta);
                    }
                    CanonicalProviderFragmentKind::Reasoning => {}
                }
                continue;
            }
            builder.flush();
            match &event.payload {
                EventV1::AssistantMessageFinished(data) => {
                    builder.committed(event.seq, &data.parts)
                }
                EventV1::TaskCompleted(data) => {
                    if crate::app::task_completed_updates_assistant_transcript(data)
                        && !builder.saw_body
                        && has_trimmed_content(&data.result_summary)
                    {
                        builder.saw_body = true;
                        builder.text(event.seq, false, &data.result_summary);
                    }
                    builder.settle_last();
                }
                EventV1::ToolCallRequested(data) => {
                    builder.saw_tool = true;
                    builder.settle_last();
                    if !committed_tools.contains(data.tool_call_id.as_str()) {
                        builder.tool(event.seq, data.tool_call_id.as_str());
                    }
                }
                _ => {}
            }
        }
        let mut parts = builder.finish(app);
        if parts.is_empty() {
            fallback(app, activity, Vec::new())
        } else {
            sync_reasoning(&mut parts, activity, thinking);
            for (seq, part) in &mut parts {
                if let TranscriptAssistantPart::Reasoning(reasoning) = part {
                    reasoning.duration_ms = app.reasoning_duration_at_seq(*seq);
                    *seq = app.reasoning_source_seq(*seq);
                }
            }
            parts
        }
    };
    if let Some(error) = &activity.error_message {
        parts.push((
            activity.last_seq,
            TranscriptAssistantPart::Error(TranscriptErrorSection {
                text: cancel_error_display_text(error, activity.duration_ms())
                    .unwrap_or_else(|| error.clone()),
            }),
        ));
    }
    let mut result = (
        Vec::with_capacity(parts.len()),
        Vec::with_capacity(parts.len()),
    );
    result.extend(
        parts
            .into_iter()
            .map(|(seq, part)| (part, TranscriptAssistantPartSourceId(seq))),
    );
    result
}

struct AssistantParts<'a> {
    activity: &'a ActivityEntry,
    thinking: bool,
    parts: Vec<SequencedPart>,
    tools: BTreeMap<String, (u64, TranscriptToolCallSection)>,
    pending_text: Option<(u64, String)>,
    saw_reasoning: bool,
    saw_body: bool,
    saw_tool: bool,
}

impl AssistantParts<'_> {
    fn text(&mut self, seq: u64, reasoning: bool, text: &str) {
        if text.is_empty() {
            return;
        }
        match (self.parts.last_mut(), reasoning) {
            (Some((_, TranscriptAssistantPart::Reasoning(last))), true) => last.text.push_str(text),
            (
                Some((
                    _,
                    TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(last)),
                )),
                false,
            ) => last.push_str(text),
            _ => self.parts.push((
                seq,
                if reasoning {
                    thought(text, None)
                } else {
                    TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(
                        text.to_owned(),
                    ))
                },
            )),
        }
    }

    fn committed(&mut self, seq: u64, parts: &[AssistantPart]) {
        self.settle_last();
        // A response's text stays above its tools without crossing
        // another response's durable sequence boundary.
        for part in parts {
            match part {
                AssistantPart::Reasoning { text } if self.thinking => {
                    self.saw_reasoning = true;
                    self.text(seq, true, text);
                }
                AssistantPart::Reasoning { .. } => {}
                AssistantPart::Text { text } => {
                    self.saw_body = true;
                    self.text(seq, false, text);
                }
                AssistantPart::ToolCall(_) => self.saw_tool = true,
            }
        }
        for part in parts {
            if let AssistantPart::ToolCall(tool) = part {
                self.settle_last();
                self.tool(seq, tool.tool_call_id.as_str());
            }
        }
        for (_, part) in &mut self.parts {
            settle(part);
        }
    }

    fn tool(&mut self, seq: u64, id: &str) {
        if let Some((_, section)) = self.tools.remove(id) {
            self.parts
                .push((seq, TranscriptAssistantPart::ToolCall(Box::new(section))));
        }
    }

    fn settle_last(&mut self) {
        if let Some((_, part)) = self.parts.last_mut() {
            settle(part);
        }
    }

    fn flush(&mut self) {
        let Some((seq, text)) = self
            .pending_text
            .take()
            .filter(|(_, text)| !text.is_empty())
        else {
            return;
        };
        let reasoning = self.thinking
            && activity_has_thinking_text(self.activity)
            && self.activity.thinking_text == text;
        self.saw_reasoning |= reasoning;
        self.saw_body |= !reasoning;
        self.text(seq, reasoning, &text);
    }

    fn finish(mut self, app: &AppState) -> Vec<SequencedPart> {
        self.flush();
        // Live suffixes belong to the current response, after committed parts.
        // They must not merge into a previous response's collapsed Thought.
        let reasoning_seq = app.uncommitted_reasoning_first_seq(self.activity);
        if let Some(text) = remainder(&self.parts, &self.activity.thinking_text, true)
            .filter(|text| self.thinking && !text.is_empty())
            .filter(|_| reasoning_seq.is_some() || (!self.saw_reasoning && !self.saw_body))
        {
            self.parts.push((
                reasoning_seq.unwrap_or(self.activity.first_seq),
                thought(text, None),
            ));
        }
        if let Some(text) = remainder(&self.parts, &self.activity.transcript_text, false)
            .filter(|text| !text.is_empty())
        {
            self.parts.push((
                app.uncommitted_text_first_seq(self.activity)
                    .unwrap_or(self.activity.last_seq),
                body(text, self.activity.status),
            ));
        }
        // Live calls can share a durable sequence number. Preserve their arrival
        // order before the remaining synthetic notification rows.
        for tool in &self.activity.tool_calls {
            if let Some((seq, section)) = self.tools.remove(&tool.tool_call_id) {
                self.parts
                    .push((seq, TranscriptAssistantPart::ToolCall(Box::new(section))));
            }
        }
        self.parts.extend(
            self.tools
                .into_values()
                .map(|(seq, section)| (seq, TranscriptAssistantPart::ToolCall(Box::new(section)))),
        );
        if self.activity.status != ActivityStatus::Streaming {
            for (_, part) in &mut self.parts {
                settle(part);
            }
        }
        // Stable sorting preserves insertion order for parts of one response.
        self.parts.sort_by_key(|(seq, _)| *seq);
        self.parts
    }
}

fn fallback(
    app: &AppState,
    activity: &ActivityEntry,
    tools: Vec<TranscriptOrderedToolCallSection>,
) -> Vec<SequencedPart> {
    let mut parts = Vec::new();
    if app.transcript_thinking_visible() && has_reasoning(activity) {
        parts.push((
            activity.first_seq,
            thought(
                &activity.thinking_text,
                app.uncommitted_reasoning_first_seq(activity)
                    .and_then(|seq| app.reasoning_duration_at_seq(seq)),
            ),
        ));
    }
    if !activity.transcript_text.is_empty() {
        parts.push((
            activity.last_seq,
            body(&activity.transcript_text, activity.status),
        ));
    }
    parts.extend(tools.into_iter().map(|tool| {
        (
            tool.first_seq,
            TranscriptAssistantPart::ToolCall(Box::new(tool.section)),
        )
    }));
    parts
}

fn thought(text: &str, duration_ms: Option<u64>) -> TranscriptAssistantPart {
    TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
        label: THINKING_TRACE_LABEL,
        duration_ms,
        text: text.to_owned(),
    })
}

fn body(text: &str, status: ActivityStatus) -> TranscriptAssistantPart {
    TranscriptAssistantPart::Body(if status == ActivityStatus::Streaming {
        TranscriptBodyBlock::StreamingRichText(text.to_owned())
    } else {
        TranscriptBodyBlock::RichText(text.to_owned())
    })
}

fn settle(part: &mut TranscriptAssistantPart) {
    if let TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text)) = part {
        *part = TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(std::mem::take(text)));
    }
}

fn has_reasoning(activity: &ActivityEntry) -> bool {
    !activity.thinking_text.trim().is_empty() || activity.thinking_duration_ms().is_some()
}

fn remainder<'a>(parts: &[SequencedPart], text: &'a str, reasoning: bool) -> Option<&'a str> {
    parts
        .iter()
        .try_fold(text, |rest, (_, part)| match (part, reasoning) {
            (TranscriptAssistantPart::Reasoning(thought), true) => {
                rest.strip_prefix(thought.text.as_str())
            }
            (
                TranscriptAssistantPart::Body(
                    TranscriptBodyBlock::RichText(body)
                    | TranscriptBodyBlock::StreamingRichText(body),
                ),
                false,
            ) => rest.strip_prefix(body.as_str()),
            _ => Some(rest),
        })
}

fn sync_reasoning(parts: &mut Vec<SequencedPart>, activity: &ActivityEntry, thinking: bool) {
    if !thinking || !has_reasoning(activity) {
        parts.retain(|(_, part)| !matches!(part, TranscriptAssistantPart::Reasoning(_)));
        return;
    }
    let reasoning =
        |(_, part): &SequencedPart| matches!(part, TranscriptAssistantPart::Reasoning(_));
    match parts.iter().filter(|part| reasoning(part)).count() {
        0 if matches!(
            activity.status,
            ActivityStatus::Done | ActivityStatus::Error
        ) && (activity.tool_calls.is_empty() || activity_has_thinking_text(activity)) =>
        {
            parts.insert(
                0,
                (activity.first_seq, thought(&activity.thinking_text, None)),
            );
        }
        1 => {
            if let Some((_, part)) = parts.iter_mut().find(|part| reasoning(part)) {
                *part = thought(&activity.thinking_text, None);
            }
        }
        2.. => {
            if let Some(rest) =
                remainder(parts, &activity.thinking_text, true).filter(|rest| !rest.is_empty())
                && let Some((_, TranscriptAssistantPart::Reasoning(last))) =
                    parts.iter_mut().rfind(|part| reasoning(part))
            {
                last.text.push_str(rest);
            }
        }
        _ => {}
    }
}

fn cancel_error_display_text(raw: &str, duration_ms: Option<u64>) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    if !["interrupted", "cancelled", "canceled", "user cancel"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return None;
    }
    let duration = match duration_ms {
        Some(ms) if ms >= 60_000 => format_duration_ms(ms),
        Some(ms) => format!(
            "{:.1}s",
            f64::from(u32::try_from(ms).unwrap_or(u32::MAX)) / 1_000.0
        ),
        None => "0.0s".to_string(),
    };
    Some(format!("Turn cancelled by user in {duration}."))
}
