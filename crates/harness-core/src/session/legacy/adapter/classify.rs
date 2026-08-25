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
            EventV1::AgentSpawned(_)
            | EventV1::AgentStopped(_)
            | EventV1::TaskScheduled(_)
            | EventV1::TaskCancelled(_)
            | EventV1::TaskCompleted(_)
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
            || self.providers.contains_key(request_id)
        {
            return Err(Self::invalid(event));
        }
        let turn_key = correlation
            .or(metadata_turn)
            .unwrap_or(request_id)
            .to_string();
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
                provider_call_id,
                finished: false,
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
                provider_id: payload.provider_id.clone(),
                model_id: payload.model_id.clone(),
            }),
        ))
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
