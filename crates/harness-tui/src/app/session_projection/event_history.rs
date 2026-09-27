//! Retained inspection borrows settled history; only the pending tail is owned here.
use std::borrow::Cow;

use super::{EventEnvelopeV1, SessionProjection};

impl SessionProjection {
    pub(crate) fn event_slices(&self) -> [&[EventEnvelopeV1]; 2] {
        if let Some(events) = &self.inspection_events {
            return [events, &[]];
        }
        let committed = self
            .canonical_projection
            .as_ref()
            .map_or(&[][..], |projection| projection.source_events());
        let pending = &self.unsettled_durable_events;
        let skipped = self.events_trimmed_count;
        [
            &committed[skipped.min(committed.len())..],
            &pending[skipped.saturating_sub(committed.len()).min(pending.len())..],
        ]
    }

    pub(crate) fn events(&self) -> impl DoubleEndedIterator<Item = &EventEnvelopeV1> + Clone {
        self.event_slices().into_iter().flatten()
    }

    pub(crate) fn event_count(&self) -> usize {
        self.event_slices().iter().map(|events| events.len()).sum()
    }

    pub(crate) fn retained_events(&self) -> Cow<'_, [EventEnvelopeV1]> {
        match self.event_slices() {
            [events, []] | [[], events] => Cow::Borrowed(events),
            _ => Cow::Owned(self.events().cloned().collect()),
        }
    }

    pub(super) fn store_inspection_event(&mut self, event: EventEnvelopeV1) -> usize {
        if self.inspection_events.is_none() {
            self.inspection_events = Some(self.events().cloned().collect());
        }
        self.inspection_events.get_or_insert_default().push(event);
        self.enforce_event_memory_cap()
    }

    pub(super) fn enforce_event_memory_cap(&mut self) -> usize {
        let trimmed = self
            .event_count()
            .saturating_sub(self.memory_caps.max_events);
        if trimmed > 0 {
            if let Some(events) = self.inspection_events.as_mut() {
                events.drain(..trimmed);
            }
            self.events_trimmed_count += trimmed;
            let oldest = self.events().next().map(|event| event.seq);
            if let Some(oldest) = oldest {
                self.reasoning_timings.retain(|seq, _| *seq >= oldest);
            }
        }
        trimmed
    }
}
