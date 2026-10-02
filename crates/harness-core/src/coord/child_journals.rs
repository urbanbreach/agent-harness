use super::*;
use crate::store::{EventEnvelopeWithoutSeqV1, Journal, JournalReader};

pub(super) struct ChildJournal {
    journal: Option<Journal>,
    source_seq: u64,
}

impl Runtime {
    pub fn publish_child_event(&mut self, event: &EventEnvelopeV1) -> Result<(), EventStoreError> {
        if matches!(&event.payload, EventV1::AgentSpawned(e) if e.parent_agent_id.is_some()) {
            self.open_child_journal(event)?;
        }
        self.append_child_event(event)
    }

    pub fn restore_child_journals(
        &mut self,
        events: &[EventEnvelopeV1],
    ) -> Result<(), EventStoreError> {
        for event in events {
            if matches!(&event.payload, EventV1::AgentSpawned(e)
                if e.parent_agent_id.is_some() && self.agents.contains_key(&e.agent_id))
            {
                self.open_child_journal(event)?;
            }
        }
        for event in events {
            self.append_child_event(event)?;
        }
        Ok(())
    }

    fn open_child_journal(&mut self, event: &EventEnvelopeV1) -> Result<(), EventStoreError> {
        let EventV1::AgentSpawned(spawn) = &event.payload else {
            return Ok(());
        };
        if self.child_journals.contains_key(&spawn.agent_id) {
            return Ok(());
        }
        let info = self
            .info
            .as_ref()
            .ok_or(EventStoreError::Invalid("run is not started"))?;
        let id = &spawn.agent_id;
        let dir = self.config.session_dir.join(id);
        let journal = Journal::open_child_projection(&self.config.session_dir, id)?;
        let parent = spawn
            .parent_agent_id
            .as_deref()
            .filter(|id| {
                self.agents
                    .get(*id)
                    .is_some_and(|a| a.info.parent_agent_id.is_some())
            })
            .unwrap_or(info.run_id.as_str());
        let saved = crate::proj::read_metadata_value(&dir)?;
        if let Some(saved) = &saved {
            if saved["run_id"] != *id
                || saved["harness_lineage"]["relationship"] != "task_child_session"
                || saved["harness_lineage"]["parent_session_id"] != parent
            {
                return Err(EventStoreError::Invalid(
                    "child journal belongs to another parent",
                ));
            }
        } else if journal.next_seq()? > 1 {
            return Err(EventStoreError::Invalid(
                "existing child journal has no lineage metadata",
            ));
        }
        if saved.is_none() {
            let mut metadata = serde_json::json!({
                "run_id": id, "run_name": id, "workspace_root": info.workspace_root,
                "created_at": event.ts, "config_digest": self.config.config_digest,
                "harness_version": self.config.harness_version,
                "recorded_runtime_context": null, "mode_source": self.config.session_mode_source,
                "harness_lineage": {
                    "relationship": "task_child_session", "parent_run_id": info.run_id,
                    "parent_session_id": parent, "child_session_id": id, "profile": spawn.profile
                }
            });
            crate::redact::redact_in_place(self.redactor.as_ref(), &mut metadata);
            crate::store::write_private_atomic(
                &dir.join(crate::proj::META_FILE_NAME),
                &serde_json::to_vec_pretty(&metadata)?,
            )?;
        }
        let mut source_seq = 0;
        for saved in JournalReader::open(
            journal.file_path(),
            std::fs::metadata(journal.file_path())?.len(),
        )? {
            if let Some(seq) = source_sequence(&saved?.event_id, info.run_id.as_str(), id) {
                source_seq = source_seq.max(seq);
            }
        }
        if journal.next_seq()? == 1 {
            let mut header = EventEnvelopeWithoutSeqV1::from(event.clone());
            header.run_id = id.clone().into();
            header.event_id = format!("child-start:{}:{}", event.run_id, event.seq);
            header.actor = super::handle::system();
            header.correlation_id = None;
            header.causation_id = None;
            header.stream_key = None;
            header.payload = EventV1::RunStarted(RunStartedEvent {
                run_name: id.clone().into(),
                workspace_root: info.workspace_root.to_string_lossy().into(),
            });
            journal.append(header)?;
        }
        if source_seq == 0 {
            let mut header = projected(event, id);
            header.actor = super::handle::system();
            header.payload = EventV1::AgentSpawned(AgentSpawnedEvent {
                parent_agent_id: None,
                ..spawn.clone()
            });
            journal.append(header)?;
            source_seq = event.seq;
        }
        self.child_journals.insert(
            id.clone(),
            ChildJournal {
                journal: None,
                source_seq,
            },
        );
        Ok(())
    }

    fn append_child_event(&mut self, event: &EventEnvelopeV1) -> Result<(), EventStoreError> {
        if self.child_journals.is_empty() {
            return Ok(());
        }
        let lifecycle = matches!(
            event.payload,
            EventV1::RunStarted(_) | EventV1::RunFinished(_) | EventV1::RunFailed(_)
        );
        let source = self.config.session_dir.join(event.run_id.as_str());
        if lifecycle {
            for (id, child) in &mut self.child_journals {
                child.append(event, id, &source, &self.config.session_dir.join(id))?;
            }
        } else if let Some((id, child)) = (match &event.payload {
            EventV1::AgentContextInitialized(initialized) => Some(&initialized.agent_id.0),
            EventV1::AgentExecutionContextChanged(changed) => Some(&changed.agent_id.0),
            _ => event.actor.agent_id.as_ref(),
        })
        .and_then(|id| self.child_journals.get_mut(id).map(|child| (id, child)))
        {
            child.append(event, id, &source, &self.config.session_dir.join(id))?;
        }
        Ok(())
    }

    pub fn write_child_selection(
        &self,
        id: &str,
        context: &crate::proj::RecordedRuntimeContext,
    ) -> Result<(), EventStoreError> {
        let dir = self.config.session_dir.join(id);
        let mut metadata = crate::proj::read_metadata_value(&dir)?
            .ok_or(EventStoreError::Invalid("child metadata is missing"))?;
        let context = serde_json::to_value(context)?;
        if metadata["recorded_runtime_context"] != context {
            metadata["recorded_runtime_context"] = context;
            crate::redact::redact_in_place(self.redactor.as_ref(), &mut metadata);
            crate::store::write_private_atomic(
                &dir.join(crate::proj::META_FILE_NAME),
                &serde_json::to_vec_pretty(&metadata)?,
            )?;
        }
        Ok(())
    }
}

impl ChildJournal {
    fn append(
        &mut self,
        event: &EventEnvelopeV1,
        id: &str,
        source: &std::path::Path,
        dir: &std::path::Path,
    ) -> Result<(), EventStoreError> {
        if event.seq <= self.source_seq {
            return Ok(());
        }
        if self.journal.is_none() {
            self.journal = Some(Journal::open_child_projection(
                dir.parent()
                    .ok_or(EventStoreError::Invalid("child has no session root"))?,
                id,
            )?);
        }
        let journal = self
            .journal
            .as_ref()
            .ok_or(EventStoreError::Invalid("child writer is closed"))?;
        crate::session_lineage::copy_session_artifacts(std::slice::from_ref(event), source, dir)
            .map_err(|e| EventStoreError::Io(std::io::Error::other(e)))?;
        let mut copy = projected(event, id);
        if let EventV1::RunStarted(start) = &mut copy.payload {
            start.run_name = id.into();
        }
        translate_cutoffs(&mut copy.payload, journal, event.run_id.as_str(), id)?;
        journal.append(copy)?;
        self.source_seq = event.seq;
        if matches!(
            event.payload,
            EventV1::RunStarted(_)
                | EventV1::RunFinished(_)
                | EventV1::RunFailed(_)
                | EventV1::TaskCompleted(_)
                | EventV1::TaskCancelled(_)
        ) {
            self.journal = None;
        }
        Ok(())
    }
}

fn projected(source: &EventEnvelopeV1, id: &str) -> EventEnvelopeWithoutSeqV1 {
    let mut copy = EventEnvelopeWithoutSeqV1::from(source.clone());
    copy.run_id = id.into();
    copy.event_id = format!("source:{}:{}", source.run_id, source.seq);
    copy.causation_id = None;
    copy.stream_key = None;
    copy
}

fn source_sequence(event_id: &str, run: &str, child: &str) -> Option<u64> {
    if let Some((source, seq)) = event_id
        .strip_prefix("source:")
        .and_then(|s| s.rsplit_once(':'))
    {
        return (source == run).then(|| seq.parse().ok()).flatten();
    }
    let (id, seq) = event_id.strip_prefix("evt_")?.rsplit_once("_mirror_")?;
    (id == child).then(|| seq.parse().ok()).flatten()
}

fn translate_cutoffs(
    payload: &mut EventV1,
    journal: &Journal,
    run: &str,
    child: &str,
) -> Result<(), EventStoreError> {
    let (cutoff, inclusive) = match payload {
        EventV1::SessionCompaction(e) => (&mut e.first_kept_event_seq, false),
        EventV1::BranchSummary(e) => (&mut e.from_event_seq, true),
        EventV1::CompactionRequested(e) => (&mut e.through_seq, true),
        EventV1::CompactionWritten(e) => (&mut e.through_seq, true),
        EventV1::CompactionApplied(e) => (&mut e.through_seq, true),
        EventV1::CompactionFailed(e) => match &mut e.through_seq {
            Some(seq) => (seq, true),
            None => return Ok(()),
        },
        _ => return Ok(()),
    };
    let mut translated = if inclusive { 0 } else { journal.next_seq()? };
    // Compaction is rare; scan its child journal instead of retaining a sequence map per event.
    for event in JournalReader::open(
        journal.file_path(),
        std::fs::metadata(journal.file_path())?.len(),
    )? {
        let event = event?;
        if let Some(seq) = source_sequence(&event.event_id, run, child) {
            if inclusive && seq <= *cutoff {
                translated = event.seq;
            }
            if !inclusive && seq >= *cutoff {
                translated = event.seq;
                break;
            }
        }
    }
    *cutoff = translated;
    Ok(())
}
