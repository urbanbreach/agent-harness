//! Live compaction progress: one generation per maintenance pass, streamed to the UI.
use super::super::*;
use crate::event::{CompactionPhase, CompactionStage, FrameThumbnail};
use std::sync::Mutex;

const PREVIEW_CHARS: usize = 512;
/// Streamed text is published in small batches so the UI can animate typing without
/// one live event per provider delta.
const PREVIEW_BATCH: usize = 24;

pub(in crate::coord) struct Progress {
    handle: CoordinatorHandle,
    actor: EventActor,
    task: String,
    agent: String,
    pub(in crate::coord) generation: u64,
    pub(in crate::coord) cancel: CancellationToken,
    /// Pairs the lifecycle events, the compaction record and the provider descriptor.
    pub(in crate::coord) operation_id: String,
    reason: String,
    state: Mutex<State>,
}

struct State {
    stage: CompactionStage,
    preview: String,
    unpublished: usize,
}

impl Progress {
    /// Registers the pass for cancellation and publishes the `preparing` stage.
    pub(in crate::coord) async fn start(
        handle: &CoordinatorHandle,
        actor: &EventActor,
        task: &str,
        agent: &str,
        reason: &str,
        parent: &CancellationToken,
    ) -> Result<Self, CoordinatorError> {
        let cancel = parent.child_token();
        let (id, token) = (agent.to_owned(), cancel.clone());
        let generation = handle
            .call(move |s| {
                s.counter += 1;
                let generation = s.counter;
                s.compacting.insert(id, (generation, token));
                Ok(generation)
            })
            .await?;
        let mut progress = Self::silent(handle, actor, task, agent, cancel);
        progress.generation = generation;
        reason.clone_into(&mut progress.reason);
        progress
            .state
            .get_mut()
            .map_err(|_| poisoned())?
            .stage
            .reason = reason.to_owned();
        progress.publish().await?;
        Ok(progress)
    }

    /// Progress for background speculation: never registered, so nothing is published.
    pub(in crate::coord) fn silent(
        handle: &CoordinatorHandle,
        actor: &EventActor,
        task: &str,
        agent: &str,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            handle: handle.clone(),
            actor: actor.clone(),
            operation_id: uuid::Uuid::now_v7().to_string(),
            task: task.to_owned(),
            agent: agent.to_owned(),
            generation: 0,
            cancel,
            reason: "threshold".to_owned(),
            state: Mutex::new(State {
                stage: CompactionStage {
                    reason: "threshold".to_owned(),
                    method: None,
                    phase: CompactionPhase::Preparing,
                    tokens_before: None,
                    tokens_after: None,
                    summarized_tokens: None,
                    kept_tokens: None,
                    fixed_tokens: None,
                    threshold_tokens: None,
                    window_tokens: None,
                    first_kept_seq: None,
                    windows_done: 0,
                    windows_total: 0,
                    frames_done: 0,
                    frames_total: 0,
                    thumbnails: Vec::new(),
                    message: None,
                },
                preview: String::new(),
                unpublished: 0,
            }),
        }
    }

    pub(in crate::coord) fn reason(&self) -> &str {
        &self.reason
    }

    fn label(stage: &CompactionStage) -> &'static str {
        match stage.phase {
            CompactionPhase::Preparing => "Selecting what to compact…",
            CompactionPhase::Summarizing => "Summarizing earlier context…",
            CompactionPhase::TurnPrefix => "Summarizing the start of this turn…",
            CompactionPhase::ShortSummary => "Writing a short summary…",
            CompactionPhase::Archiving => "Printing archive frames…",
            CompactionPhase::Remote => "Server-side compaction…",
            CompactionPhase::Handoff => "Writing handoff…",
            CompactionPhase::Shaking => "Shaking out heavy output…",
            CompactionPhase::Committing => "Committing…",
            CompactionPhase::Done => "Compacted",
            CompactionPhase::Failed => "Compaction failed",
            CompactionPhase::Cancelled => "Compaction cancelled",
            CompactionPhase::Skipped => "Nothing to compact",
        }
    }

    async fn publish(&self) -> Result<(), CoordinatorError> {
        let (stage, preview) = {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            state.unpublished = 0;
            let preview = if state.preview.is_empty() {
                Self::label(&state.stage).to_owned()
            } else {
                state.preview.clone()
            };
            (state.stage.clone(), preview)
        };
        self.send(Some(preview), Some(stage)).await
    }

    async fn send(
        &self,
        preview: Option<String>,
        stage: Option<CompactionStage>,
    ) -> Result<(), CoordinatorError> {
        let (actor, task, agent, generation, reason) = (
            self.actor.clone(),
            self.task.clone(),
            self.agent.clone(),
            self.generation,
            self.reason.clone(),
        );
        self.handle
            .call(move |s| {
                if s.compacting
                    .get(&agent)
                    .is_none_or(|(current, _)| *current != generation)
                {
                    return Ok(());
                }
                s.live(
                    actor,
                    task,
                    LiveEventV1::CompactionProgress {
                        agent_id: agent,
                        generation,
                        trigger_reason: reason,
                        preview,
                        stage,
                    },
                )
            })
            .await
    }

    /// Mutates the stage and publishes it. A phase change restarts the streamed preview.
    pub(in crate::coord) async fn update(
        &self,
        change: impl FnOnce(&mut CompactionStage),
    ) -> Result<(), CoordinatorError> {
        {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            let before = (state.stage.phase, state.stage.method);
            change(&mut state.stage);
            if before != (state.stage.phase, state.stage.method) {
                state.preview.clear();
            }
        }
        self.publish().await
    }

    pub(in crate::coord) async fn phase(
        &self,
        method: Option<CompactionMethod>,
        phase: CompactionPhase,
    ) -> Result<(), CoordinatorError> {
        self.update(|stage| {
            stage.method = method;
            stage.phase = phase;
            stage.message = None;
        })
        .await
    }

    /// Appends streamed model text to the preview tail.
    pub(in crate::coord) async fn preview(&self, delta: &str) -> Result<(), CoordinatorError> {
        let due = {
            let mut state = self.state.lock().map_err(|_| poisoned())?;
            state.preview.push_str(delta);
            let excess = state.preview.chars().count().saturating_sub(PREVIEW_CHARS);
            if excess > 0 {
                let cut = state
                    .preview
                    .char_indices()
                    .nth(excess)
                    .map_or(state.preview.len(), |(index, _)| index);
                state.preview.drain(..cut);
            }
            state.unpublished += delta.len();
            state.unpublished >= PREVIEW_BATCH || delta.contains('\n')
        };
        if due {
            self.publish().await?;
        }
        Ok(())
    }

    pub(in crate::coord) async fn thumbnail(
        &self,
        thumbnail: FrameThumbnail,
        done: u32,
        total: u32,
    ) -> Result<(), CoordinatorError> {
        self.update(|stage| {
            stage.thumbnails.push(thumbnail);
            stage.frames_done = done;
            stage.frames_total = total;
        })
        .await
    }

    /// Publishes the terminal stage, then the end marker, and releases the cancel slot.
    pub(in crate::coord) async fn finish(
        &self,
        phase: CompactionPhase,
        message: Option<String>,
    ) -> Result<(), CoordinatorError> {
        self.update(|stage| {
            stage.phase = phase;
            stage.message = message;
        })
        .await?;
        self.send(None, None).await?;
        let (agent, generation) = (self.agent.clone(), self.generation);
        self.handle
            .call(move |s| {
                if s.compacting
                    .get(&agent)
                    .is_some_and(|(current, _)| *current == generation)
                {
                    s.compacting.remove(&agent);
                }
                Ok(())
            })
            .await
    }
}

fn poisoned() -> CoordinatorError {
    CoordinatorError::Invalid("compaction progress state was poisoned".into())
}
