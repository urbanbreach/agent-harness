use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::clock::Clock;
use crate::redact::{redact_value, Redactor};

pub const SCHEMA_VERSION: u16 = 1;
const DEFAULT_EVENT_ID_PREFIX: &str = "evt";
const MAX_SUMMARY_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelopeV1 {
    pub schema_version: u16,
    pub event_id: String,
    pub seq: u64,
    pub run_id: String,
    pub mono_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    pub actor: EventActor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,
    pub payload: EventV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventActor {
    pub kind: ActorKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

impl EventActor {
    pub fn new(kind: ActorKind, agent_id: Option<String>) -> Self {
        Self { kind, agent_id }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Supervisor,
    Worker,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventContext {
    pub seq: u64,
    pub event_id: Option<String>,
    pub actor: EventActor,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub stream_key: Option<String>,
}

impl EventContext {
    pub fn new(seq: u64, actor: EventActor) -> Self {
        Self {
            seq,
            event_id: None,
            actor,
            correlation_id: None,
            causation_id: None,
            stream_key: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data", rename_all = "snake_case")]
pub enum EventV1 {
    RunStarted(RunStartedEvent),
    RunFinished(RunFinishedEvent),
    RunFailed(RunFailedEvent),
    AgentSpawned(AgentSpawnedEvent),
    AgentStopped(AgentStoppedEvent),
    TaskScheduled(TaskScheduledEvent),
    TaskCancelled(TaskCancelledEvent),
    TaskCompleted(TaskCompletedEvent),
    TaskResultLate(TaskResultLateEvent),
    StaleDetected(StaleDetectedEvent),
    UserMessageSubmitted(UserMessageSubmittedEvent),
    ProviderRequestStarted(ProviderRequestStartedEvent),
    ProviderStreamDelta(ProviderStreamDeltaEvent),
    ProviderRequestFinished(ProviderRequestFinishedEvent),
    ToolCallRequested(ToolCallRequestedEvent),
    ToolCallStarted(ToolCallStartedEvent),
    ToolCallFinished(ToolCallFinishedEvent),
    PermissionRequested(PermissionRequestedEvent),
    PermissionResolved(PermissionResolvedEvent),
    EditProposed(EditProposedEvent),
    EditApplied(EditAppliedEvent),
    EditRejected(EditRejectedEvent),
    ArtifactWritten(ArtifactWrittenEvent),
    PolicyViolationDetected(PolicyViolationDetectedEvent),
    UiIntentReceived(UiIntentReceivedEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStartedEvent {
    pub run_name: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFinishedEvent {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailedEvent {
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpawnedEvent {
    pub agent_id: String,
    pub profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStoppedEvent {
    pub agent_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskScheduledEvent {
    pub task_id: String,
    pub state: TaskScheduleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskScheduleState {
    Queued,
    Started,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolIdentityMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskLineageMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_model_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ExecutionTimingMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_mono_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_mono_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionStatus {
    Succeeded,
    Failed,
    Skipped,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookExecutionMetadata {
    pub hook_name: String,
    pub status: HookExecutionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventArtifactRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolCallMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias_source_tool_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<TaskLineageMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_refs: Vec<EventArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<ExecutionTimingMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_executions: Vec<HookExecutionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskCompletionMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<TaskLineageMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<ExecutionTimingMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_executions: Vec<HookExecutionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCancelledEvent {
    pub task_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCompletedEvent {
    pub task_id: String,
    pub result_summary: String,
    pub result_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<TaskCompletionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskResultLateEvent {
    pub task_id: String,
    pub result_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleDetectedEvent {
    pub task_id: String,
    pub stale_for_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserMessageSubmittedEvent {
    pub request_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestStartedEvent {
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamDeltaEvent {
    pub request_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestFinishedEvent {
    pub request_id: String,
    pub finish_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallRequestedEvent {
    pub tool_call_id: String,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallStartedEvent {
    pub tool_call_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallFinishedEvent {
    pub tool_call_id: String,
    pub status: ToolCallStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ToolCallMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequestedEvent {
    pub permission_id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
}

pub struct PermissionRequestedArgs {
    pub permission_id: String,
    pub kind: String,
    pub tool_call_id: Option<String>,
    pub summary: String,
    pub request_digest: String,
    pub timeout_ms: u64,
    pub default_decision: PermissionDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionResolvedEvent {
    pub permission_id: String,
    pub decision: PermissionDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditProposedEvent {
    pub edit_id: String,
    pub path: String,
    pub summary: String,
    pub patch_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditAppliedEvent {
    pub edit_id: String,
    pub path: String,
    pub new_file_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_rel_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditRejectedEvent {
    pub edit_id: String,
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactWrittenEvent {
    pub path: String,
    pub digest: String,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_metadata: Option<ToolIdentityMetadata>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyViolationDetectedEvent {
    pub policy: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiIntentReceivedEvent {
    pub intent: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

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
    run_id: String,
}

impl<'a, C: Clock + ?Sized, R: Redactor + ?Sized> EventBuilder<'a, C, R> {
    pub fn new(clock: &'a C, redactor: &'a R, run_id: impl Into<String>) -> Self {
        Self {
            clock,
            redactor,
            run_id: run_id.into(),
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
            run_name: run_name.into(),
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
        tool_call_id: impl Into<String>,
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
        truncate_summary(&as_text, MAX_SUMMARY_CHARS)
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

fn truncate_summary(summary: &str, max_chars: usize) -> String {
    if summary.chars().count() <= max_chars {
        return summary.to_string();
    }

    let mut truncated: String = summary.chars().take(max_chars).collect();
    truncated.push('…');
    truncated
}

fn value_digest(value: &Value) -> String {
    let canonical = canonicalize_json(value);
    let canonical_bytes = serde_json::to_vec(&canonical).unwrap_or_else(|_| b"null".to_vec());
    let digest = blake3::hash(&canonical_bytes);
    digest.to_hex().chars().take(12).collect()
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

#[cfg(test)]
mod tests {
    use super::{
        ActorKind, EventActor, EventBuilder, EventContext, EventV1, PermissionDecision,
        PermissionRequestedArgs, ToolCallRequestedEvent,
    };
    use crate::clock::FakeClock;
    use crate::redact::DefaultRedactor;
    use serde_json::json;

    #[test]
    fn run_started_snapshot_is_stable_in_deterministic_mode() {
        let clock = FakeClock::new();
        clock.advance(42);
        let redactor = DefaultRedactor::default();
        let builder = EventBuilder::new(&clock, &redactor, "run_123");

        let mut context = EventContext::new(
            1,
            EventActor::new(ActorKind::Supervisor, Some("agent-supervisor".to_string())),
        );
        context.stream_key = Some("run:run_123".to_string());

        let envelope = builder
            .run_started(context, "golden_path", "/workspace/project")
            .expect("build run started envelope");

        insta::assert_json_snapshot!("run_started_envelope_v1", envelope);
    }

    #[test]
    fn permission_requested_snapshot_is_stable_in_deterministic_mode() {
        let clock = FakeClock::new();
        clock.advance(128);
        let redactor = DefaultRedactor::default();
        let builder = EventBuilder::new(&clock, &redactor, "run_123");

        let mut context = EventContext::new(
            2,
            EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        );
        context.correlation_id = Some("toolcall_001".to_string());
        context.stream_key = Some("permission:perm_001".to_string());

        let envelope = builder
            .permission_requested(
                context,
                PermissionRequestedArgs {
                    permission_id: "perm_001".to_string(),
                    kind: "edit".to_string(),
                    tool_call_id: Some("toolcall_001".to_string()),
                    summary: "Apply patch to file with Bearer abc.def".to_string(),
                    request_digest: "req_90ac2e1e".to_string(),
                    timeout_ms: 30_000,
                    default_decision: PermissionDecision::Deny,
                },
            )
            .expect("build permission requested envelope");

        insta::assert_json_snapshot!("permission_requested_envelope_v1", envelope);
    }

    #[test]
    fn tool_call_requested_uses_redacted_summary_and_digest() {
        let clock = FakeClock::new();
        let redactor = DefaultRedactor::default();
        let builder = EventBuilder::new(&clock, &redactor, "run_123");

        let args = json!({
            "cmd": "curl https://example.invalid",
            "auth": "Bearer secret.value",
            "api_key": "sk-ABCDE12345ABCDE",
        });

        let envelope = builder
            .tool_call_requested(
                EventContext::new(
                    3,
                    EventActor::new(ActorKind::Worker, Some("agent-worker".to_string())),
                ),
                "toolcall_002",
                "shell.run",
                &args,
                None,
            )
            .expect("build tool call requested envelope");

        let EventV1::ToolCallRequested(ToolCallRequestedEvent {
            args_summary,
            args_digest,
            ..
        }) = envelope.payload
        else {
            panic!("expected tool call requested payload")
        };

        assert!(!args_summary.contains("Bearer secret.value"));
        assert!(!args_summary.contains("sk-ABCDE12345ABCDE"));
        assert!(args_summary.contains("Bearer [REDACTED]"));
        assert!(args_summary.contains("[REDACTED_API_KEY]"));
        assert_eq!(args_digest.len(), 12);
    }
}
