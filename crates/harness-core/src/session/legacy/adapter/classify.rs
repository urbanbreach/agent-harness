use super::*;

impl LegacyBoundary {
    #[expect(
        deprecated,
        reason = "the V1 compatibility boundary must classify deprecated V1 variants"
    )]
    pub(super) fn classify(
        &mut self,
        event: &EventEnvelopeV1,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        if self.terminal {
            if matches!(event.payload, EventV1::RunStarted(_)) {
                self.terminal = false;
                return Ok(Self::fact(event, LegacyFactKind::Noop));
            }
            return Err(Self::invalid(event));
        }
        let fact = match &event.payload {
            EventV1::RunStarted(_) => {
                if self.run_started {
                    return Err(Self::invalid(event));
                }
                self.run_started = true;
                Self::fact(event, LegacyFactKind::RunStarted)
            }
            EventV1::SessionTitleUpdated(payload) => {
                Self::fact(event, LegacyFactKind::Title(payload.title.clone()))
            }
            EventV1::RunFinished(_) => {
                self.terminal = true;
                Self::fact(
                    event,
                    LegacyFactKind::RunTerminal {
                        run: RunStatus::Completed,
                        session: SessionStatus::Completed,
                    },
                )
            }
            EventV1::RunFailed(_) => {
                self.terminal = true;
                Self::fact(
                    event,
                    LegacyFactKind::RunTerminal {
                        run: RunStatus::Failed,
                        session: SessionStatus::Failed,
                    },
                )
            }
            EventV1::UserMessageSubmitted(payload) => self.user_message(event, payload)?,
            EventV1::PromptAttachmentsSubmitted(payload) => self.attachments(event, payload)?,
            EventV1::ProviderRequestStarted(payload) => self.provider_started(event, payload)?,
            EventV1::ProviderStreamDelta(payload) => self.assistant_part(
                event,
                payload.request_id.as_str(),
                AssistantPart::Text {
                    text: payload.delta.clone(),
                },
            )?,
            EventV1::ProviderReasoningDelta(payload) => self.assistant_part(
                event,
                payload.request_id.as_str(),
                AssistantPart::Reasoning {
                    text: payload.delta.clone(),
                },
            )?,
            EventV1::ProviderRequestFinished(payload) => self.provider_finished(event, payload)?,
            EventV1::AssistantMessageFinished(payload) => {
                self.assistant_finished(event, payload)?
            }
            EventV1::SessionCompaction(payload) => {
                if payload.first_kept_event_seq == 0 {
                    return Err(Self::invalid(event));
                }
                let mut compaction = super::super::LegacyCompactionFact::from(payload);
                if compaction.current_intent.is_none() {
                    compaction.current_intent =
                        self.current_intent_by_agent.get(&payload.agent_id).cloned();
                }
                Self::fact(event, LegacyFactKind::Compaction(compaction))
            }
            EventV1::BranchSummary(payload) => {
                if payload.from_event_seq == 0 || payload.from_event_seq >= event.seq {
                    return Err(Self::invalid(event));
                }
                Self::fact(
                    event,
                    LegacyFactKind::BranchSummary(payload.summary.clone()),
                )
            }
            EventV1::ToolCallRequested(payload) => self.tool_requested(event, payload)?,
            EventV1::ToolCallStarted(payload) => {
                self.tool_started(event, payload.tool_call_id.as_str())?
            }
            EventV1::ToolCallFinished(payload) => self.tool_finished(event, payload)?,
            EventV1::TaskScheduled(payload) => self.task_scheduled(event, payload),
            EventV1::TaskCompleted(payload) => {
                self.task_terminal(payload.task_id.as_str());
                Self::fact(event, LegacyFactKind::Noop)
            }
            EventV1::TaskCancelled(payload) => self.task_cancelled(event, payload),
            EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::TaskResultLate(_)
            | EventV1::BackgroundTaskNotification(_)
            | EventV1::StaleDetected(_)
            | EventV1::CompactionRequested(_)
            | EventV1::CompactionWritten(_)
            | EventV1::CompactionApplied(_)
            | EventV1::CompactionFailed(_)
            | EventV1::PermissionRequested(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::PermissionResolved(_)
            | EventV1::EditProposed(_)
            | EventV1::EditApplied(_)
            | EventV1::EditRejected(_)
            | EventV1::ArtifactWritten(_)
            | EventV1::PolicyViolationDetected(_)
            | EventV1::WorkspaceSnapshot(_)
            | EventV1::WorkspaceReverted(_) => self.unsupported(event),
            EventV1::UiIntentReceived(payload) => {
                if let Some(agent_id) = event.actor.agent_id.as_ref() {
                    self.current_intent_by_agent
                        .insert(agent_id.clone(), payload.clone());
                }
                Self::fact(event, LegacyFactKind::CurrentIntent)
            }
        };
        Ok(fact)
    }

    fn user_message(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::UserMessageSubmittedEvent,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let request_id = payload.request_id.as_str();
        if Self::non_empty(request_id).is_none()
            || Self::correlation(event).is_some_and(|value| value != request_id)
            || !self.seen_users.insert(request_id.to_string())
        {
            return Err(Self::invalid(event));
        }
        Ok(Self::fact(
            event,
            LegacyFactKind::User {
                request_id: request_id.to_string(),
                text: payload.text.clone(),
            },
        ))
    }

    fn attachments(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::PromptAttachmentsSubmittedEvent,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let request_id = payload.request_id.as_str();
        if Self::non_empty(request_id).is_none()
            || Self::correlation(event).is_some_and(|value| value != request_id)
        {
            return Err(Self::invalid(event));
        }
        if !self.users.contains(request_id) {
            self.warnings
                .push(LegacyWarning::MissingAttachmentAssociation {
                    request_id: request_id.to_string(),
                });
        }
        Ok(Self::fact(
            event,
            LegacyFactKind::Attachments {
                request_id: request_id.to_string(),
                values: payload.attachments.clone(),
            },
        ))
    }

    fn provider_started(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::ProviderRequestStartedEvent,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let request_id = payload.request_id.as_str();
        let metadata_turn = payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.turn_id.as_deref())
            .and_then(Self::non_empty);
        let correlation = Self::correlation(event);
        if Self::non_empty(request_id).is_none()
            || Self::non_empty(&payload.provider_id).is_none()
            || Self::non_empty(&payload.model_id).is_none()
            || correlation
                .zip(metadata_turn)
                .is_some_and(|(left, right)| left != right)
        {
            return Err(Self::invalid(event));
        }
        let active_turn = event.actor.agent_id.as_ref().and_then(|agent_id| {
            self.active_agent_turn_by_agent
                .get(agent_id)
                .map(|(_, turn_key)| turn_key.as_str())
        });
        let turn_key = metadata_turn
            .or(active_turn)
            .or(correlation)
            .unwrap_or(request_id)
            .to_string();
        let existing = self.providers.get(request_id).cloned();
        if existing.as_ref().is_some_and(|existing| {
            existing.finished
                || existing.assistant_finished
                || existing.turn_key != turn_key
                || existing.owner_agent_id != event.actor.agent_id
                || existing.event_correlation.as_deref() != correlation
        }) {
            return Err(Self::invalid(event));
        }
        let inferred_user_text =
            if existing.is_none() && self.represented_user_turns.insert(turn_key.clone()) {
                let prompt = Self::non_empty(&payload.prompt_summary).ok_or_else(|| {
                    LegacyAdapterError::MissingUserMessage {
                        request_id: request_id.to_string(),
                    }
                })?;
                if prompt.ends_with('…') {
                    return Err(LegacyAdapterError::TruncatedUserPromptSummary {
                        request_id: request_id.to_string(),
                    });
                }
                Some(prompt.to_string())
            } else {
                None
            };
        if correlation.is_none() && metadata_turn.is_none() {
            self.warnings.push(LegacyWarning::InferredTurnIdentity {
                correlation_id: None,
            });
        }
        let provider_call_id = payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.provider_call_id.clone());
        if provider_call_id
            .as_deref()
            .is_some_and(|value| Self::non_empty(value).is_none())
        {
            return Err(Self::invalid(event));
        }
        self.providers.insert(
            request_id.to_string(),
            ProviderRelationship {
                turn_key: turn_key.clone(),
                owner_agent_id: event.actor.agent_id.clone(),
                event_correlation: correlation.map(str::to_string),
                provider_call_id,
                finished: false,
                stop_reason: None,
                assistant_finished: false,
            },
        );
        self.latest_provider_by_turn
            .insert(turn_key.clone(), request_id.to_string());
        Ok(Self::fact(
            event,
            LegacyFactKind::ProviderStarted(ProviderStartFact {
                request_id: request_id.to_string(),
                turn_key,
                inferred_user_text,
                provider_id: payload.provider_id.clone(),
                model_id: payload.model_id.clone(),
                runtime_selection: payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.runtime_selection.clone()),
            }),
        ))
    }

    fn task_scheduled(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::TaskScheduledEvent,
    ) -> LegacyFact {
        if payload
            .queue_key
            .as_deref()
            .is_some_and(|queue| queue.starts_with("provider_model:"))
        {
            if let (Some(agent_id), Some(turn_key)) =
                (event.actor.agent_id.as_ref(), Self::correlation(event))
            {
                self.active_agent_turn_by_agent.insert(
                    agent_id.clone(),
                    (payload.task_id.to_string(), turn_key.to_string()),
                );
                self.agent_turn_agent_by_task
                    .insert(payload.task_id.to_string(), agent_id.clone());
            }
        }
        Self::fact(event, LegacyFactKind::Noop)
    }

    fn task_terminal(&mut self, task_id: &str) {
        let Some(agent_id) = self.agent_turn_agent_by_task.remove(task_id) else {
            return;
        };
        if self
            .active_agent_turn_by_agent
            .get(&agent_id)
            .is_some_and(|(active_task_id, _)| active_task_id == task_id)
        {
            self.active_agent_turn_by_agent.remove(&agent_id);
        }
    }

    fn task_cancelled(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::TaskCancelledEvent,
    ) -> LegacyFact {
        let is_agent_turn = matches!(
            payload.task_scope,
            Some(crate::event::TaskTerminalScope::AgentTurn)
        ) || self
            .agent_turn_agent_by_task
            .contains_key(payload.task_id.as_str());
        let turn_key = Self::correlation(event).map(str::to_string);
        let provider_error = turn_key
            .as_ref()
            .and_then(|turn| self.latest_provider_by_turn.get(turn))
            .and_then(|request_id| self.providers.get(request_id))
            .and_then(|provider| provider.stop_reason.as_deref())
            == Some("error");
        self.task_terminal(payload.task_id.as_str());
        let Some(turn_key) = turn_key.filter(|_| is_agent_turn) else {
            return Self::fact(event, LegacyFactKind::Noop);
        };
        let (status, stage) = cancelled_turn_status_stage(provider_error, &payload.reason);
        Self::fact(
            event,
            LegacyFactKind::TurnCancelled(super::super::facts::TurnCancelledFact {
                turn_key,
                status: status.to_string(),
                stage: stage.to_string(),
                reason: payload.reason.clone(),
            }),
        )
    }

    fn assistant_part(
        &self,
        event: &EventEnvelopeV1,
        request_id: &str,
        part: AssistantPart,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let relationship = self.provider_relationship(event, request_id)?;
        if relationship.finished || relationship.assistant_finished {
            return Err(Self::invalid(event));
        }
        Ok(Self::fact(
            event,
            LegacyFactKind::AssistantPart {
                request_id: request_id.to_string(),
                part,
            },
        ))
    }
}

fn cancelled_turn_status_stage(provider_error: bool, reason: &str) -> (&'static str, &'static str) {
    if provider_error {
        return ("failed", "provider_error");
    }
    if reason.contains("overflow persisted after checkpoint compaction") {
        return ("failed", "overflow_retry_failed");
    }
    if reason.contains("failed closed") {
        return ("failed", "tool_failure");
    }
    if reason.contains("critical lifecycle hook failed") || reason.contains("lifecycle hook failed")
    {
        return ("failed", "hook_failure");
    }
    if reason.contains("agent turn exceeded profile max_iters=") {
        return ("aborted", "max_iters");
    }
    ("aborted", "cancelled")
}
