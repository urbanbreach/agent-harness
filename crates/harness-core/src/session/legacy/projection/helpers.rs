use super::*;

impl SessionProjector<'_> {
    pub(super) fn apply_title(&mut self, fact: &LegacyFact, title: &str) {
        self.push_entry(SessionEntry {
            id: self
                .namespace
                .entry_id(fact.sequence, &fact.event_id, "session_metadata"),
            parent_id: None,
            turn_id: None,
            run_id: self.run_id.clone(),
            payload: SessionEntryPayload::SessionMetadata {
                title: Some(title.to_string()),
            },
        });
        self.push_record(CanonicalRecordKind::SessionMetadataUpdated {
            metadata: SessionMetadata {
                title: Some(title.to_string()),
                custom: BTreeMap::new(),
            },
        });
    }

    pub(super) fn apply_assistant(
        &mut self,
        start: &super::super::facts::ProviderStartFact,
    ) -> Result<(), LegacyAdapterError> {
        let Some(assistant) = self.index.assistants.remove(&start.request_id) else {
            return Err(LegacyAdapterError::InvalidIdentityRelationship {
                event_id: start.request_id.clone(),
            });
        };
        if !assistant.finished || assistant.parts.is_empty() {
            self.warnings
                .push(LegacyWarning::MissingFinalAssistantContent {
                    request_id: start.request_id.clone(),
                });
        }
        self.push_entry(SessionEntry {
            id: assistant.entry_id,
            parent_id: None,
            turn_id: Some(self.namespace.turn_id(&assistant.turn_key)),
            run_id: self.run_id.clone(),
            payload: SessionEntryPayload::AssistantMessage {
                parts: assistant.parts.into_iter().map(|(_, part)| part).collect(),
                provenance: Some(ProviderProvenance {
                    provider_id: assistant.provider_id,
                    model_id: assistant.model_id,
                    request_id: self.namespace.provider_request_id(&start.request_id),
                    response_id: assistant.response_id,
                    stop_reason: assistant.stop_reason,
                    usage: assistant.usage,
                }),
            },
        });
        Ok(())
    }

    pub(super) fn apply_compaction(
        &mut self,
        fact: &LegacyFact,
        summary: &str,
        first_kept_event_seq: u64,
    ) {
        let first_kept = self
            .index
            .source_entries
            .get(&first_kept_event_seq)
            .cloned();
        if first_kept_event_seq >= fact.sequence || first_kept.is_none() {
            self.warnings
                .push(LegacyWarning::MissingCompactionBoundary {
                    first_kept_event_seq,
                });
            return;
        }
        if let Some(first_kept_entry_id) = first_kept {
            self.push_entry(SessionEntry {
                id: self
                    .namespace
                    .entry_id(fact.sequence, &fact.event_id, "compaction_summary"),
                parent_id: None,
                turn_id: None,
                run_id: self.run_id.clone(),
                payload: SessionEntryPayload::CompactionSummary {
                    summary: summary.to_string(),
                    first_kept_entry_id,
                },
            });
        }
    }

    pub(super) fn push_entry(&mut self, mut entry: SessionEntry) {
        entry.parent_id.clone_from(&self.active_leaf);
        self.active_leaf = Some(entry.id.clone());
        self.push_record(CanonicalRecordKind::EntryCommitted { entry });
    }

    pub(super) fn push_record(&mut self, kind: CanonicalRecordKind) {
        self.records.push(CanonicalRecord {
            session_id: self.session_id.clone(),
            sequence: RecordSequence::new(self.records.len() as u64 + 1),
            kind,
        });
    }

    pub(super) fn finish(
        self,
        facts: &[LegacyFact],
    ) -> Result<LegacySessionSnapshot, LegacyAdapterError> {
        let session = reducer::replay(self.session_id, &self.records)?;
        Ok(LegacySessionSnapshot {
            session,
            provenance: LegacyProvenance {
                schema_version: SCHEMA_VERSION,
                source_run_id: self.run_id.clone(),
                source_event_count: facts.len(),
            },
            warnings: self.warnings,
            audit_timeline: facts
                .iter()
                .map(|fact| LegacyAuditReference {
                    sequence: fact.sequence,
                    event_id: fact.event_id.clone(),
                })
                .collect(),
        })
    }
}
