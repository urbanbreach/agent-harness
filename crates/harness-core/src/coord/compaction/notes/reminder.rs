//! The near-threshold reminder: once per window, a model-only note asks the agent to bring
//! its notebook up to date before the window rolls over. Nothing is summarized ahead of time.
use super::super::maintain::Measure;
use crate::coord::{context::Context, turn::Worker, *};
use harness_compaction::{policy, NoteKind, Origin};
use harness_providers::{CompletionMessage, MessageRole};

const REMINDER: &str = include_str!("../../../../prompts/notes/reminder.md");

/// Which window a context is in: the latest compaction, else the first entry, else none.
fn window_key(context: &Context) -> String {
    let body = context.body_start();
    match context.entries.get(body) {
        Some(entry) if entry.compaction.is_some() => format!("compaction:{}", entry.seq),
        Some(entry) => format!("entry:{}", entry.seq),
        None => "empty".to_owned(),
    }
}

/// The agent's prompt generation: how many turns it has started. A reminder is only valid
/// for the prompt it was queued under.
fn prompt_generation(runtime: &Runtime, agent: &str) -> u64 {
    runtime
        .agents
        .get(agent)
        .map_or(0, |a| u64::from(a.prompt_turns))
}

/// Whether `tokens` lies in the lead band `[threshold - lead, threshold)`.
fn in_band(tokens: u32, threshold: u32) -> bool {
    tokens >= threshold.saturating_sub(policy::speculation_lead(threshold)) && tokens < threshold
}

impl Worker {
    /// Queues the reminder once per window while occupancy is in the lead band. It needs
    /// automatic maintenance on and a known window, and does not depend on the async
    /// setting, the method order or before-compaction hooks. The window is recorded when
    /// the reminder is queued, not when it is delivered, and a new prompt does not reopen it.
    pub(in crate::coord) async fn queue_notes_reminder(
        &mut self,
        context: &Context,
        measure: &Measure,
    ) -> Result<(), CoordinatorError> {
        if !self.compaction.enabled
            || measure.window == 0
            || !in_band(measure.tokens, measure.threshold)
        {
            return Ok(());
        }
        let (agent, key) = (
            self.actor.agent_id.clone().unwrap_or_default(),
            window_key(context),
        );
        self.handle
            .call(move |s| {
                let generation = prompt_generation(s, &agent);
                let state = s.notes.entry(agent).or_default();
                if state.reminded_window.as_deref() != Some(key.as_str()) {
                    state.reminded_window = Some(key);
                    state.reminder_queued = true;
                    state.reminder_generation = generation;
                }
                Ok(())
            })
            .await
    }

    /// At an aside boundary, places a queued reminder in context once. A reminder queued
    /// for an earlier prompt is dropped rather than delivered late.
    pub(in crate::coord) async fn drain_notes_reminder(
        &mut self,
        messages: &mut Context,
    ) -> Result<(), CoordinatorError> {
        if !self.compaction.notes_windows {
            return Ok(());
        }
        let (agent, request) = (
            self.actor.agent_id.clone().unwrap_or_default(),
            self.turn.id.clone(),
        );
        let delivered = self
            .handle
            .call(move |s| {
                s.check_task(&request)?;
                let generation = prompt_generation(s, &agent);
                let state = s.notes.entry(agent.clone()).or_default();
                if !std::mem::take(&mut state.reminder_queued)
                    || state.reminder_generation != generation
                {
                    return Ok(None);
                }
                let text = REMINDER.trim_end().to_owned();
                let event = s.emit(
                    EventActor::new(ActorKind::Worker, Some(agent)),
                    Some(request.clone()),
                    EventV1::RuntimeReminder(RuntimeReminderEvent {
                        request_id: request.into(),
                        kind: RuntimeReminderKind::NotesWindow,
                        text: text.clone(),
                        source: None,
                    }),
                )?;
                Ok(Some((event.seq, text)))
            })
            .await?;
        if let Some((seq, text)) = delivered {
            messages.push(
                CompletionMessage::text(MessageRole::User, text),
                seq,
                Some(&self.turn.id),
            );
            if let Some(entry) = messages.entries.last_mut() {
                entry.origin = Origin::Host;
                entry.note_kind = Some(NoteKind::NotesWindow);
            }
        }
        Ok(())
    }
}

/// Forgets the reminded window after a rollover, so the next window can remind again.
pub(in crate::coord) fn clear(runtime: &mut Runtime, agent: &str) {
    let state = runtime.notes.entry(agent.to_owned()).or_default();
    state.reminded_window = None;
    state.reminder_queued = false;
}

#[cfg(test)]
mod tests {
    use super::in_band;

    #[test]
    fn lead_band_is_half_open_at_the_documented_bounds() {
        // Threshold 50,000: lead clamps to 8,192, so the band opens at 41,808.
        for (tokens, expected) in [
            (41_807, false),
            (41_808, true),
            (49_999, true),
            (50_000, false),
        ] {
            assert_eq!(in_band(tokens, 50_000), expected, "{tokens} tokens");
        }
    }
}
