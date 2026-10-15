//! Best-effort summaries of the conversation suffix abandoned by a rewind.
use super::{context::Context, runtime::*, *};

mod generate;

use generate::BranchJob;

impl Runtime {
    pub(super) fn start_branch_summary(&mut self, boundary: u64) -> Result<bool, CoordinatorError> {
        self.start_branch_summary_with(boundary, None)
    }

    /// Starts summarizing the suffix from `boundary`. `custom_instructions`, when truthy,
    /// replaces the branch template.
    pub(super) fn start_branch_summary_with(
        &mut self,
        boundary: u64,
        custom_instructions: Option<String>,
    ) -> Result<bool, CoordinatorError> {
        let events = crate::store::read_events(&self.info()?.events_path)?;
        let active = crate::conversation_rewind::active_events(&events);
        let primary = active.iter().find_map(|e| match &e.payload {
            EventV1::AgentSpawned(e) if e.parent_agent_id.is_none() => Some(e.agent_id.clone()),
            _ => None,
        });
        let owner = active
            .iter()
            .find(|e| e.seq == boundary)
            .and_then(|e| e.actor.agent_id.clone())
            .or(primary.clone());
        let Some(owner) = owner else {
            return Ok(false);
        };
        let agent = self
            .agents
            .get(&owner)
            .ok_or_else(|| CoordinatorError::UnknownAgent(owner.clone()))?;
        let target = agent.target.clone().unwrap_or_else(|| {
            let model = crate::agent::AgentModelRef::parse(&agent.info.model_ref);
            super::resume::restored_target(
                &crate::session::CanonicalRuntimeSelection {
                    provider_id: model.provider_id,
                    model_id: model.model_id,
                    ..Default::default()
                },
                None,
                &self.config,
            )
        });
        let abandoned: Vec<_> = active
            .iter()
            .filter(|e| e.seq >= boundary && !matches!(e.payload, EventV1::SessionCompaction(_)))
            .cloned()
            .collect();
        let mut context = super::history::messages(
            &abandoned,
            &owner,
            primary.as_deref() == Some(&owner),
            "",
            &self.info()?.run_dir,
        )?;
        // A reader that cannot read archived reasoning never sees it in summary text.
        let strip = super::compaction::archive::excludes_archived_reasoning(
            &target.provider,
            &target.model,
        );
        for event in active.iter().filter(|e| e.seq >= boundary) {
            if let EventV1::SessionCompaction(e) = &event.payload
                && e.agent_id == owner
            {
                let text = match &e.short_summary {
                    Some(short) => format!("{}\n\n{short}", e.summary),
                    None => e.summary.clone(),
                };
                let text = if strip {
                    harness_compaction::archive::strip_reasoning(&text)
                } else {
                    text
                };
                let mut entry = super::context::summary_entry(super::context::CompactionState {
                    summary: text,
                    short_summary: None,
                    method: e
                        .method
                        .unwrap_or(harness_compaction::CompactionMethod::Soft),
                    tokens_before: e.tokens_before,
                    tokens_after: e.tokens_after,
                    read_files: e.read_files.clone(),
                    modified_files: e.modified_files.clone(),
                    warning: None,
                    archive: None,
                    native: None,
                    commit_seq: Default::default(),
                    details: Default::default(),
                    from_hook: Default::default(),
                    history_rewrite_at_ms: Default::default(),
                    preserve_data: Default::default(),
                });
                entry.seq = event.seq;
                context.entries.push(entry);
            }
        }
        context.entries.sort_by_key(|e| e.seq);
        if context.entries.is_empty() {
            return Ok(false);
        }
        let workspace = agent.cwd.clone();
        let id = self.id("branch-summary")?;
        let handle = CoordinatorHandle {
            tx: self.tx.upgrade().ok_or(CoordinatorError::Closed)?,
            _owner: None,
        };
        let cancel = CancellationToken::new();
        self.compacting
            .insert(owner.clone(), (self.counter, cancel.clone()));
        let actor = EventActor::new(ActorKind::Worker, Some(owner.clone()));
        let job = BranchJob {
            handle,
            provider: Arc::clone(&self.config.provider),
            permits: Arc::clone(&self.providers),
            actor: actor.clone(),
            id: id.clone(),
            cancel: cancel.clone(),
            target,
            reserve: self.config.compaction.branch_summary.reserve_tokens,
            workspace,
            session: self.info()?.run_id.to_string(),
            custom_instructions,
        };
        let from_event_seq = active.last().map_or(boundary, |e| e.seq);
        let worker_id = id.clone();
        let task = self.jobs.spawn(async move {
            let outcome = generate::generate(job, context).await;
            Completion::BranchSummary {
                id: worker_id.clone(),
                result: outcome.into_event(owner, from_event_seq, &worker_id),
            }
        });
        self.running.insert(
            id,
            Job {
                join_id: Some(task.id()),
                actor,
                kind: JobKind::BranchSummary,
                parent: None,
                cancellation: cancel,
                reason: None,
                hooks: Vec::new(),
            },
        );
        Ok(true)
    }

    pub(super) fn branch_summary_warning(&mut self, error: &CoordinatorError) {
        if let (Some(store), Some(info)) = (&self.store, &self.info) {
            store.publish_live(LiveEventEnvelope {
                event_id: format!("branch-summary-warning:{}", self.last_seq),
                run_id: info.run_id.clone(),
                mono_ms: self.clock.mono_ms(),
                ts: None,
                actor: super::handle::system(),
                correlation_id: None,
                causation_id: None,
                stream_key: None,
                payload: LiveEventV1::RuntimeWarning {
                    message: self
                        .redactor
                        .redact_text(&format!("Branch summary failed: {error}")),
                },
            });
        }
    }

    pub(super) fn finish_branch_summary(
        &mut self,
        id: String,
        result: Result<BranchSummaryEvent, CoordinatorError>,
    ) -> Result<(), CoordinatorError> {
        if let Some(job) = self.running.remove(&id)
            && let Some(agent) = job.actor.agent_id
        {
            self.compacting.remove(&agent);
        }
        // An abort or an empty selection commits nothing quietly; only failures warn.
        if let Err(error) = &result
            && !matches!(error, CoordinatorError::Cancelled(_))
        {
            self.branch_summary_warning(error);
        }
        self.rewind_summary = Some(result);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
