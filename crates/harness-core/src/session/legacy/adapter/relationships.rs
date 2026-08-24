use super::*;

impl LegacyBoundary {
    pub(super) fn provider_finished(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::ProviderRequestFinishedEvent,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let request_id = payload.request_id.as_str();
        let relationship = self.provider_relationship(event, request_id)?.clone();
        let metadata_turn = payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.turn_id.as_deref())
            .and_then(Self::non_empty);
        let finished_call_id = payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.provider_call_id.as_deref());
        let response_id = payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.provider_response_id.clone());
        if relationship.finished
            || metadata_turn.is_some_and(|turn| turn != relationship.turn_key)
            || relationship
                .provider_call_id
                .as_deref()
                .zip(finished_call_id)
                .is_some_and(|(started, finished)| started != finished)
            || finished_call_id.is_some_and(|value| Self::non_empty(value).is_none())
            || response_id
                .as_deref()
                .is_some_and(|value| Self::non_empty(value).is_none())
        {
            return Err(Self::invalid(event));
        }
        if let Some(value) = self.providers.get_mut(request_id) {
            value.finished = true;
        }
        let stop_reason = payload
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.provider_stop_reason.clone())
            .unwrap_or_else(|| payload.finish_reason.clone());
        Ok(Self::fact(
            event,
            LegacyFactKind::ProviderFinished(ProviderFinishFact {
                request_id: request_id.to_string(),
                response_id,
                stop_reason,
                usage: payload.usage.clone(),
            }),
        ))
    }

    pub(super) fn assistant_finished(
        &mut self,
        event: &EventEnvelopeV1,
        request_id: &str,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let relationship = self.provider_relationship(event, request_id)?;
        if !relationship.finished || relationship.assistant_finished {
            return Err(Self::invalid(event));
        }
        if let Some(value) = self.providers.get_mut(request_id) {
            value.assistant_finished = true;
        }
        Ok(Self::fact(
            event,
            LegacyFactKind::AssistantFinished {
                request_id: request_id.to_string(),
            },
        ))
    }

    pub(super) fn tool_requested(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::ToolCallRequestedEvent,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let tool_call_id = payload.tool_call_id.as_str();
        let Some(turn_key) = Self::correlation(event) else {
            return Err(Self::invalid(event));
        };
        let Some(request_id) = self.latest_provider_by_turn.get(turn_key).cloned() else {
            return Err(Self::invalid(event));
        };
        if Self::non_empty(tool_call_id).is_none()
            || Self::non_empty(&payload.tool_id).is_none()
            || Self::non_empty(&payload.args_digest).is_none()
            || self.tools.contains_key(tool_call_id)
        {
            return Err(Self::invalid(event));
        }
        self.tools.insert(
            tool_call_id.to_string(),
            ToolRelationship {
                request_id: request_id.clone(),
                turn_key: turn_key.to_string(),
                finished: false,
            },
        );
        Ok(Self::fact(
            event,
            LegacyFactKind::AssistantPart {
                request_id,
                part: AssistantPart::ToolCall(AssistantToolCall {
                    tool_call_id: payload.tool_call_id.clone(),
                    tool_id: payload.tool_id.clone(),
                    args_summary: payload.args_summary.clone(),
                    args_digest: payload.args_digest.clone(),
                    provider_call_id: None,
                }),
            },
        ))
    }

    pub(super) fn tool_started(
        &self,
        event: &EventEnvelopeV1,
        tool_call_id: &str,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let Some(relationship) = self.tools.get(tool_call_id) else {
            return Err(Self::invalid(event));
        };
        if Self::correlation(event)
            .is_some_and(|value| value != relationship.turn_key && value != tool_call_id)
        {
            return Err(Self::invalid(event));
        }
        Ok(Self::fact(event, LegacyFactKind::Noop))
    }

    pub(super) fn tool_finished(
        &mut self,
        event: &EventEnvelopeV1,
        payload: &crate::event::ToolCallFinishedEvent,
    ) -> Result<LegacyFact, LegacyAdapterError> {
        let tool_call_id = payload.tool_call_id.as_str();
        let Some(relationship) = self.tools.get(tool_call_id).cloned() else {
            return Err(Self::invalid(event));
        };
        if relationship.finished
            || Self::correlation(event)
                .is_some_and(|value| value != relationship.turn_key && value != tool_call_id)
        {
            return Err(Self::invalid(event));
        }
        if let Some(value) = self.tools.get_mut(tool_call_id) {
            value.finished = true;
        }
        let status = match payload.status {
            crate::event::ToolCallStatus::Succeeded => ToolResultStatus::Succeeded,
            crate::event::ToolCallStatus::Failed => ToolResultStatus::Failed,
        };
        Ok(Self::fact(
            event,
            LegacyFactKind::ToolFinished(ToolFinishFact {
                request_id: relationship.request_id,
                tool_call_id: payload.tool_call_id.clone(),
                status,
                output_summary: payload.output_summary.clone(),
                output_digest: payload.output_digest.clone(),
                output_json: payload.output_json.clone(),
            }),
        ))
    }
}
