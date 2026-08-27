use super::*;

impl SessionProjector<'_> {
    pub(super) fn apply_turn_cancelled(
        &mut self,
        fact: &LegacyFact,
        cancelled: &super::super::facts::TurnCancelledFact,
    ) {
        let partial = self
            .index
            .partial_text_by_turn
            .get(&cancelled.turn_key)
            .filter(|text| !text.trim().is_empty())
            .map_or("(none)", String::as_str);
        let text = format!(
            "Harness preserved an incomplete provider turn for continuity. Do not treat it as a completed answer.\nStatus: {}\nStage: {}\nReason: {}\nPartial assistant output:\n{partial}",
            cancelled.status, cancelled.stage, cancelled.reason
        );
        self.push_entry(SessionEntry {
            id: self
                .namespace
                .entry_id(fact.sequence, &fact.event_id, "turn_cancelled"),
            parent_id: None,
            turn_id: Some(self.namespace.turn_id(&cancelled.turn_key)),
            run_id: self.run_id.clone(),
            payload: SessionEntryPayload::AssistantMessage {
                parts: vec![crate::session::AssistantPart::Text { text }],
                provenance: None,
            },
        });
    }

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
        let is_final_provider_for_turn = self
            .index
            .last_provider_by_turn
            .get(&start.turn_key)
            .is_some_and(|request_id| request_id == &start.request_id);
        let Some(assistant) = self.index.assistants.remove(&start.request_id) else {
            return Ok(());
        };
        let has_tool_call = assistant
            .parts
            .iter()
            .any(|(_, part)| matches!(part, crate::session::AssistantPart::ToolCall(_)));
        if !is_final_provider_for_turn && !has_tool_call {
            return Ok(());
        }
        if !assistant.finished || assistant.parts.is_empty() {
            self.warnings
                .push(LegacyWarning::MissingFinalAssistantContent {
                    request_id: start.request_id.clone(),
                });
        }
        if !assistant.finished
            && (!self.preserve_incomplete_assistant || assistant.parts.is_empty())
        {
            return Ok(());
        }
        let provenance = match assistant.provenance {
            Some(mut provenance) => {
                if provenance.runtime_selection.is_none() {
                    provenance.runtime_selection = start.runtime_selection.clone();
                }
                Some(provenance)
            }
            None => Some(ProviderProvenance {
                provider_id: assistant.provider_id,
                model_id: assistant.model_id,
                request_id: self.namespace.provider_request_id(&start.request_id),
                response_id: assistant.response_id,
                stop_reason: assistant.stop_reason,
                usage: assistant.usage,
                runtime_selection: start.runtime_selection.clone(),
            }),
        };
        self.push_entry(SessionEntry {
            id: assistant.entry_id,
            parent_id: None,
            turn_id: Some(self.namespace.turn_id(&assistant.turn_key)),
            run_id: self.run_id.clone(),
            payload: SessionEntryPayload::AssistantMessage {
                parts: assistant.parts.into_iter().map(|(_, part)| part).collect(),
                provenance: provenance.map(Box::new),
            },
        });
        Ok(())
    }

    pub(super) fn apply_compaction(
        &mut self,
        fact: &LegacyFact,
        compaction: &LegacyCompactionFact,
    ) {
        let first_kept_entry_id = compaction.first_kept_entry_id.clone().or_else(|| {
            self.index
                .source_entries
                .get(&compaction.first_kept_event_seq)
                .cloned()
        });
        if compaction.first_kept_event_seq >= fact.sequence || first_kept_entry_id.is_none() {
            self.warnings
                .push(LegacyWarning::MissingCompactionBoundary {
                    first_kept_event_seq: compaction.first_kept_event_seq,
                });
            return;
        }
        if let Some(first_kept_entry_id) = first_kept_entry_id {
            self.push_entry(SessionEntry {
                id: self
                    .namespace
                    .entry_id(fact.sequence, &fact.event_id, "compaction_summary"),
                parent_id: None,
                turn_id: None,
                run_id: self.run_id.clone(),
                payload: SessionEntryPayload::CompactionSummary {
                    summary: compaction.summary.clone(),
                    first_kept_entry_id,
                    tokens_after: compaction.tokens_after,
                    summary_usage: compaction.summary_usage.clone(),
                    summary_provider_id: compaction.summary_provider_id.clone(),
                    summary_model_id: compaction.summary_model_id.clone(),
                    preserved_state: Some(Box::new(crate::session::CompactionPreservedState {
                        read_files: compaction.read_files.clone(),
                        modified_files: compaction.modified_files.clone(),
                        current_intent: compaction.current_intent.clone(),
                    })),
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
