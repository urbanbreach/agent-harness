use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use crate::clock::Clock;
use crate::digest::digest12_json;
use crate::redact::{redact_value, Redactor};
use crate::text::truncate_with_ellipsis;

use super::{
    EventContext, EventEnvelopeV1, EventV1, PermissionRequestedArgs, PermissionRequestedEvent,
    RunStartedEvent, ToolCallMetadata, ToolCallRequestedEvent, SCHEMA_VERSION,
};

const DEFAULT_EVENT_ID_PREFIX: &str = "evt";
const MAX_SUMMARY_CHARS: usize = 512;

#[derive(Debug, Error)]
pub enum EventBuildError {
    #[error("failed to serialize event envelope for redaction: {0}")]
    SerializeEnvelope(#[source] serde_json::Error),
    #[error("failed to deserialize redacted event envelope: {0}")]
    DeserializeEnvelope(#[source] serde_json::Error),
}

pub struct EventBuilder<'a, C: Clock + ?Sized, R: Redactor + ?Sized> {
    clock: &'a C,
    redactor: &'a R,
    run_id: crate::ids::RunId,
}

impl<'a, C: Clock + ?Sized, R: Redactor + ?Sized> EventBuilder<'a, C, R> {
    pub fn new(clock: &'a C, redactor: &'a R, run_id: impl Into<String>) -> Self {
        Self {
            clock,
            redactor,
            run_id: crate::ids::RunId::from(run_id.into()),
        }
    }

    pub fn build(
        &self,
        context: EventContext,
        payload: EventV1,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let envelope = EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: context
                .event_id
                .unwrap_or_else(|| default_event_id(context.seq)),
            seq: context.seq,
            run_id: self.run_id.clone(),
            mono_ms: self.clock.mono_ms(),
            ts: self.clock.system_time_rfc3339(),
            actor: context.actor,
            correlation_id: context.correlation_id,
            causation_id: context.causation_id,
            stream_key: context.stream_key,
            payload,
        };

        self.redact_envelope(envelope)
    }

    pub fn run_started(
        &self,
        context: EventContext,
        run_name: impl Into<String>,
        workspace_root: impl Into<String>,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let payload = EventV1::RunStarted(RunStartedEvent {
            run_name: crate::ids::RunName::from(run_name.into()),
            workspace_root: workspace_root.into(),
        });
        self.build(context, payload)
    }

    pub fn permission_requested(
        &self,
        context: EventContext,
        args: PermissionRequestedArgs,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let PermissionRequestedArgs {
            permission_id,
            kind,
            tool_call_id,
            summary,
            request_digest,
            timeout_ms,
            default_decision,
        } = args;
        let payload = EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id,
            kind,
            tool_call_id,
            summary,
            request_digest,
            timeout_ms,
            default_decision,
        });
        self.build(context, payload)
    }

    pub fn tool_call_requested(
        &self,
        context: EventContext,
        tool_call_id: impl Into<crate::ids::ToolCallId>,
        tool_id: impl Into<String>,
        raw_args: &Value,
        metadata: Option<ToolCallMetadata>,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let args_summary = self.summarize_and_redact(raw_args);
        let args_digest = value_digest(raw_args);
        let payload = EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.into(),
            args_summary,
            args_digest,
            metadata,
        });

        self.build(context, payload)
    }

    fn summarize_and_redact(&self, value: &Value) -> String {
        let redacted = redact_value(self.redactor, value);
        let as_text = serde_json::to_string(&redacted).unwrap_or_else(|_| "null".to_string());
        truncate_with_ellipsis(&as_text, MAX_SUMMARY_CHARS)
    }

    fn redact_envelope(
        &self,
        envelope: EventEnvelopeV1,
    ) -> Result<EventEnvelopeV1, EventBuildError> {
        let value = serde_json::to_value(&envelope).map_err(EventBuildError::SerializeEnvelope)?;
        let redacted = redact_value(self.redactor, &value);
        serde_json::from_value(redacted).map_err(EventBuildError::DeserializeEnvelope)
    }
}

fn default_event_id(seq: u64) -> String {
    format!("{DEFAULT_EVENT_ID_PREFIX}-{seq:020}")
}

fn value_digest(value: &Value) -> String {
    let canonical = canonicalize_json(value);
    digest12_json(&canonical)
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut ordered = serde_json::Map::new();
            for (key, value) in map.iter().collect::<BTreeMap<_, _>>() {
                ordered.insert(key.clone(), canonicalize_json(value));
            }
            Value::Object(ordered)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}
