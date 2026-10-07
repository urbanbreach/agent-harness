//! Redaction-aware batching of live provider deltas.
use super::*;

#[derive(Default)]
pub(super) struct LiveOutput {
    pending: BTreeMap<(u8, String), (LiveEventV1, String, bool)>,
}
impl LiveOutput {
    pub(super) fn push(
        &mut self,
        mut payload: LiveEventV1,
        redactor: &dyn crate::redact::Redactor,
    ) -> Option<LiveEventV1> {
        let (key, delta) = match &mut payload {
            LiveEventV1::ProviderTextDelta { delta, .. } => ((0, String::new()), delta),
            LiveEventV1::ProviderReasoningDelta { delta, .. } => ((1, String::new()), delta),
            LiveEventV1::ProviderToolInputDelta {
                tool_call_id,
                delta,
                ..
            } => ((2, tool_call_id.to_string()), delta),
            _ => return Some(payload),
        };
        let delta = std::mem::take(delta);
        let tool_name = match &payload {
            LiveEventV1::ProviderToolInputDelta { tool_name, .. } => tool_name.clone(),
            _ => None,
        };
        let (template, pending, held) =
            self.pending
                .entry(key)
                .or_insert((payload, String::new(), false));
        if let LiveEventV1::ProviderToolInputDelta {
            tool_name: current, ..
        } = template
            && tool_name.is_some()
        {
            current.clone_from(&tool_name);
        }
        pending.push_str(&delta);
        if *held {
            return None;
        }
        let prefix = redactor.streaming_prefix(pending);
        let safe: String = pending.drain(..prefix).collect();
        // ponytail: defer a suspicious tail over 4 KiB instead of rescanning it per delta.
        // The response-wide 4 MiB budget also bounds all retained tails together.
        *held = pending.len() > 4096;
        (!safe.is_empty() || tool_name.is_some()).then(|| with_delta(template.clone(), safe))
    }
    pub(super) fn finish(self, redactor: &dyn crate::redact::Redactor) -> Vec<LiveEventV1> {
        self.pending
            .into_values()
            .filter_map(|(payload, pending, _)| {
                (!pending.is_empty()).then(|| with_delta(payload, redactor.redact_text(&pending)))
            })
            .collect()
    }
}
fn with_delta(mut payload: LiveEventV1, text: String) -> LiveEventV1 {
    match &mut payload {
        LiveEventV1::ProviderTextDelta { delta, .. }
        | LiveEventV1::ProviderReasoningDelta { delta, .. }
        | LiveEventV1::ProviderToolInputDelta { delta, .. } => *delta = text,
        _ => {}
    }
    payload
}
