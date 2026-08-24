// allow: SIZE_OK — recovery prompt context (error state + session resume + context entry assembly)
//! Headless session recovery helpers shared by CLI entrypoints.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use harness_core::crash_recovery::{
    inspect_previous_crash, resolve_crash_recovery_action, CrashRecoveryAction,
    CrashRecoveryApplyResult, PreviousCrashReport,
};
use harness_core::event::{ActorKind, EventEnvelopeV1, EventV1};
use harness_core::proj::{
    inspect_resume_plan, project_run_summary, ResumePlan, RunStatus, SessionModeSource,
};
use serde::Serialize;

use crate::cli_io::{load_events_from_run_dir, EVENTS_FILE_NAME};
use crate::cli_labels::provider_model_label;
use crate::replay::inspect_session_catalog;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryPromptContextEntry {
    pub seq: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryChildSessionEntry {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_child_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_terminal_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_terminal_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_delivered_turn_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    pub hook_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecoveryArtifactEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionRecoverySummary {
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_name: Option<String>,
    pub run_dir: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RunStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub total_events: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_model: Option<String>,
    pub mode: SessionModeSource,
    pub resumable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_disabled_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_agent_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub pending_permissions: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tasks_in_flight: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub prompt_context: Vec<RecoveryPromptContextEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub child_sessions: Vec<RecoveryChildSessionEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artifacts: Vec<RecoveryArtifactEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continue_hint: Option<String>,
    pub previous_crash_detected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_crash: Option<PreviousCrashReport>,
    /// Human-readable recovery guidance when a previous crash was detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_message: Option<String>,
    /// Structured recovery action for CLI/TUI operators when a previous crash was detected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_action: Option<CrashRecoveryAction>,
    /// Operator-facing command/hint for the structured recovery action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_action_hint: Option<String>,
}

/// Canonical `harness sessions reopen --json` response (Packet 3.4).
///
/// Exactly one typed envelope: the recovery summary nests under `summary` and
/// crash-recovery details (when present) under `crash_recovery`. The legacy
/// shape duplicated every summary field at the top level next to `summary`;
/// that duplication is removed — consumers read `summary.*` and the optional
/// `crash_recovery` object only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionReopenJsonResponse {
    pub summary: SessionRecoverySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crash_recovery: Option<CrashRecoveryApplyResult>,
}

pub fn resolve_session_run_dir(
    session_selector: &str,
    default_session_dir: &Path,
) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(session_selector);
    if candidate.exists() {
        return ensure_run_dir(candidate);
    }

    let direct_child = default_session_dir.join(session_selector);
    if direct_child.exists() {
        return ensure_run_dir(direct_child);
    }

    let matches = inspect_session_catalog(default_session_dir)?
        .into_iter()
        .filter(|entry| {
            entry.catalog.run_id == session_selector
                || entry.run_dir.file_name().and_then(|name| name.to_str())
                    == Some(session_selector)
        })
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => Err(format!(
            "no saved session matched `{session_selector}` under {}",
            default_session_dir.display()
        )),
        [entry] => ensure_run_dir(entry.run_dir.clone()),
        _ => Err(format!(
            "multiple saved sessions matched `{session_selector}` under {}; pass an explicit run directory path",
            default_session_dir.display()
        )),
    }
}

pub fn inspect_session_recovery(run_dir: &Path) -> Result<SessionRecoverySummary, String> {
    let events = load_events_from_run_dir(run_dir).map_err(|err| err.to_string())?;
    if events.is_empty() {
        return Err(format!("no events found in {}", run_dir.display()));
    }

    let run_summary = project_run_summary(events.iter()).map_err(|err| err.to_string())?;
    let resume_plan = inspect_resume_plan(run_dir);
    let catalog = inspect_single_catalog_entry(run_dir)?;
    let run_name = latest_run_name(&events).or_else(|| catalog.run_name.clone());
    let resume_agent_id = if catalog.is_resumable {
        Some(select_resume_agent_id(
            &resume_plan,
            &events,
            &catalog.run_id,
        )?)
    } else {
        None
    };
    let previous_crash = inspect_previous_crash(run_dir);
    let previous_crash_detected = previous_crash.previous_crash_detected;
    let recovery_action =
        previous_crash_detected.then(|| resolve_crash_recovery_action(catalog.is_resumable));
    let recovery_action_hint = recovery_action.map(|action| action.operator_hint(&catalog.run_id));
    let continue_mock_flag =
        matches!(catalog.mode_source, SessionModeSource::InteractiveMock).then_some(" --mock");

    Ok(SessionRecoverySummary {
        run_id: catalog.run_id.clone(),
        run_name,
        run_dir: run_dir.to_path_buf(),
        workspace_root: catalog.workspace_root,
        status: catalog.status,
        last_error: run_summary.last_error,
        total_events: run_summary.counts.total_events,
        profile: catalog.profile_preset,
        provider_model: catalog.provider_model,
        mode: catalog.mode_source,
        resumable: catalog.is_resumable,
        resume_disabled_reason: catalog.resume_disabled_reason,
        resume_agent_id: resume_agent_id.clone(),
        pending_permissions: run_summary.pending_permissions.into_iter().collect(),
        tasks_in_flight: run_summary.tasks_in_flight.into_iter().collect(),
        prompt_context: collect_prompt_context(&events),
        child_sessions: collect_child_sessions(&resume_plan),
        artifacts: collect_artifacts(&resume_plan),
        continue_hint: resume_agent_id.map(|_| {
            format!(
                "harness prompt{} --resume {} --text \"<next prompt>\"",
                continue_mock_flag.unwrap_or_default(),
                catalog.run_id,
            )
        }),
        previous_crash_detected,
        recovery_message: previous_crash.recovery_message.clone(),
        previous_crash: previous_crash_detected.then_some(previous_crash),
        recovery_action,
        recovery_action_hint,
    })
}

pub fn latest_run_name(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::SessionTitleUpdated(data) => Some(data.title.clone()),
        EventV1::RunStarted(data) => Some(data.run_name.to_string()),
        _ => None,
    })
}

pub fn select_resume_agent_id(
    resume_plan: &ResumePlan,
    historical_events: &[EventEnvelopeV1],
    run_id: &str,
) -> Result<String, String> {
    if resume_plan.known_agents.is_empty() {
        return Err(format!(
            "continue session requires at least one agent binding for {run_id}"
        ));
    }

    most_recent_conversational_agent_id(historical_events, &resume_plan.known_agents)
        .or_else(|| most_recent_known_agent_spawn_id(historical_events, &resume_plan.known_agents))
        .ok_or_else(|| {
            format!(
                "continue session requires a deterministically targetable conversational agent for {run_id}"
            )
        })
}

fn ensure_run_dir(run_dir: PathBuf) -> Result<PathBuf, String> {
    if !run_dir.is_dir() {
        return Err(format!("{} is not a session directory", run_dir.display()));
    }
    if !run_dir.join(EVENTS_FILE_NAME).exists() {
        return Err(format!(
            "{} does not contain an {EVENTS_FILE_NAME} session log",
            run_dir.display(),
        ));
    }
    Ok(run_dir)
}

fn inspect_single_catalog_entry(
    run_dir: &Path,
) -> Result<harness_core::proj::SessionCatalogEntry, String> {
    let parent_dir = run_dir.parent().ok_or_else(|| {
        format!(
            "failed to resolve parent session directory for {}",
            run_dir.display()
        )
    })?;
    inspect_session_catalog(parent_dir)?
        .into_iter()
        .find(|entry| entry.run_dir == run_dir)
        .map(|entry| entry.catalog)
        .ok_or_else(|| {
            format!(
                "failed to inspect session catalog entry for {}",
                run_dir.display()
            )
        })
}

fn collect_prompt_context(events: &[EventEnvelopeV1]) -> Vec<RecoveryPromptContextEntry> {
    let mut entries = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::UserMessageSubmitted(payload) => Some(RecoveryPromptContextEntry {
                seq: event.seq,
                kind: "user_message".to_string(),
                request_id: Some(payload.request_id.to_string()),
                agent_id: None,
                text: payload.text.clone(),
            }),
            EventV1::ProviderRequestStarted(payload) => Some(RecoveryPromptContextEntry {
                seq: event.seq,
                kind: "provider_prompt".to_string(),
                request_id: Some(payload.request_id.to_string()),
                agent_id: event.actor.agent_id.clone(),
                text: payload.prompt_summary.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let keep_from = entries.len().saturating_sub(6);
    entries.drain(0..keep_from);
    entries
}

fn collect_child_sessions(resume_plan: &ResumePlan) -> Vec<RecoveryChildSessionEntry> {
    let mut entries = resume_plan
        .child_sessions
        .iter()
        .map(|(agent_id, child)| RecoveryChildSessionEntry {
            agent_id: agent_id.clone(),
            profile: resume_plan
                .known_agents
                .get(agent_id)
                .cloned()
                .or_else(|| child.profile.clone()),
            provider_model: provider_model_label(
                child.provider_id.as_deref(),
                child.model_id.as_deref(),
            ),
            latest_child_request_id: child.latest_child_request_id.clone(),
            parent_session_id: child.parent_session_id.clone(),
            parent_tool_call_id: child.parent_tool_call_id.clone(),
            parent_task_id: child.parent_task_id.clone(),
            parent_request_id: child.parent_request_id.clone(),
            terminal_state: child.terminal_state.as_ref().map(|state| {
                serde_json::to_string(state)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string()
            }),
            terminal_reason: child.terminal_reason.clone(),
            notification_status: child
                .background_notification
                .as_ref()
                .map(|notification| notification.status.as_str().to_string()),
            notification_summary: child
                .background_notification
                .as_ref()
                .map(|notification| notification.summary.clone()),
            notification_terminal_event_id: child
                .background_notification
                .as_ref()
                .map(|notification| notification.terminal_event_id.clone()),
            notification_terminal_task_id: child
                .background_notification
                .as_ref()
                .map(|notification| notification.terminal_task_id.clone()),
            notification_delivered_turn_request_id: child
                .background_notification
                .as_ref()
                .and_then(|notification| notification.delivered_turn_request_id.clone()),
            elapsed_ms: child.timing.as_ref().and_then(|timing| timing.elapsed_ms),
            hook_count: child.hook_executions.len(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.agent_id.cmp(&right.agent_id));
    entries
}

fn collect_artifacts(resume_plan: &ResumePlan) -> Vec<RecoveryArtifactEntry> {
    let mut entries = resume_plan
        .session_artifacts
        .values()
        .map(|artifact| RecoveryArtifactEntry {
            tool_call_id: artifact.tool_call_id.as_ref().map(|id| id.to_string()),
            tool_id: artifact.tool_id.clone(),
            kind: artifact.artifact_kind.clone(),
            path: artifact.path.clone(),
            digest: artifact.digest.clone(),
            parent_tool_call_id: artifact.parent_tool_call_id.clone(),
            parent_task_id: artifact.parent_task_id.clone(),
            parent_request_id: artifact.parent_request_id.clone(),
            child_session_id: artifact.child_session_id.clone(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.tool_call_id.cmp(&right.tool_call_id))
    });
    entries
}

pub(crate) fn most_recent_conversational_agent_id(
    historical_events: &[EventEnvelopeV1],
    known_agents: &BTreeMap<String, String>,
) -> Option<String> {
    historical_events.iter().rev().find_map(|event| {
        let conversational_payload = matches!(
            &event.payload,
            EventV1::ProviderRequestStarted(_)
                | EventV1::ProviderStreamDelta(_)
                | EventV1::ProviderRequestFinished(_)
                | EventV1::AssistantMessageFinished(_)
                | EventV1::TaskCompleted(_)
                | EventV1::TaskCancelled(_)
        );
        if !conversational_payload || event.actor.kind != ActorKind::Worker {
            return None;
        }

        event
            .actor
            .agent_id
            .as_ref()
            .filter(|agent_id| known_agents.contains_key(*agent_id))
            .cloned()
    })
}

fn most_recent_known_agent_spawn_id(
    historical_events: &[EventEnvelopeV1],
    known_agents: &BTreeMap<String, String>,
) -> Option<String> {
    historical_events.iter().rev().find_map(|event| {
        let EventV1::AgentSpawned(data) = &event.payload else {
            return None;
        };
        known_agents
            .contains_key(&data.agent_id)
            .then(|| data.agent_id.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::event::{
        ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
        ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, SessionTitleUpdatedEvent,
        SCHEMA_VERSION,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn envelope(seq: u64, agent_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{}", seq),
            seq,
            run_id: "test-run".into(),
            mono_ms: seq * 100,
            ts: None,
            actor: EventActor::new(
                if agent_id.is_some() {
                    ActorKind::Worker
                } else {
                    ActorKind::System
                },
                agent_id.map(|s| s.to_string()),
            ),
            correlation_id: None,
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn write_events(run_dir: &Path, events: &[EventEnvelopeV1]) {
        let events_path = run_dir.join(EVENTS_FILE_NAME);
        let mut content = String::new();
        for event in events {
            content.push_str(&serde_json::to_string(event).unwrap());
            content.push('\n');
        }
        fs::write(events_path, content).unwrap();
    }

    fn setup_session_dir(root: &Path, run_id: &str) -> PathBuf {
        let run_dir = root.join(run_id);
        fs::create_dir_all(&run_dir).unwrap();
        run_dir
    }

    #[test]
    fn test_inspect_session_recovery_happy_path() {
        // arrange
        // act
        // assert
        let root = tempdir().unwrap();
        // create a sessions directory so that run_dir.parent() works and inspect_session_catalog can find it
        let sessions_dir = root.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let run_dir = setup_session_dir(&sessions_dir, "test-run");

        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                2,
                Some("agent-1"),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent-1".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                Some("agent-1"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ];
        write_events(&run_dir, &events);

        let summary = inspect_session_recovery(&run_dir).unwrap();
        assert_eq!(summary.run_id, "test-run");
        assert_eq!(summary.run_name, Some("interactive".to_string()));
        assert!(summary.resumable);
        assert_eq!(summary.resume_agent_id, Some("agent-1".to_string()));
        assert_eq!(summary.total_events, 4);
        assert!(!summary.previous_crash_detected);
        assert!(summary.previous_crash.is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_inspect_session_recovery_recovery_action_hint_when_resumable() {
        // arrange
        // act
        // assert
        // Given: resumable session (spawn + provider request + finish) with stale lock
        let root = tempdir().unwrap();
        let sessions_dir = root.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let run_dir = setup_session_dir(&sessions_dir, "test-run");
        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                2,
                Some("agent-1"),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent-1".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                Some("agent-1"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ];
        write_events(&run_dir, &events);
        fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap();

        // When
        let summary = inspect_session_recovery(&run_dir).unwrap();

        // Then: recovery_message + structured recovery_action are present and actionable
        let message = summary.recovery_message.expect("recovery_message");
        assert!(summary.previous_crash_detected);
        assert!(summary.resumable, "fixture must remain catalog-resumable");
        assert!(message.contains("Previous crash detected"));
        assert!(message.contains("sessions inspect"));
        assert!(message.contains("Stale writer lock"));
        assert_eq!(
            summary.recovery_action,
            Some(CrashRecoveryAction::ResumeWithPrompt)
        );
        let hint = summary
            .recovery_action_hint
            .as_deref()
            .expect("recovery_action_hint");
        assert!(hint.contains("harness prompt --resume"));
        assert!(hint.contains("test-run"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_inspect_session_recovery_reports_previous_crash_from_stale_lock() {
        // arrange
        // act
        // assert
        let root = tempdir().unwrap();
        let sessions_dir = root.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let run_dir = setup_session_dir(&sessions_dir, "test-run");

        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                2,
                Some("agent-1"),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent-1".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ];
        write_events(&run_dir, &events);
        fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap();

        let summary = inspect_session_recovery(&run_dir).unwrap();
        assert!(summary.previous_crash_detected);
        let report = summary.previous_crash.expect("crash report");
        assert!(report.stale_writer_lock);
        let message = summary.recovery_message.expect("recovery_message");
        assert!(message.contains("Previous crash detected"));
        assert!(message.contains("Stale writer lock"));
        assert!(summary.recovery_action.is_some());
        assert!(summary.recovery_action_hint.is_some());
    }

    #[test]
    fn test_inspect_session_recovery_no_events() {
        // arrange
        // act
        // assert
        let root = tempdir().unwrap();
        let run_dir = setup_session_dir(root.path(), "test-run");
        write_events(&run_dir, &[]);

        let err = inspect_session_recovery(&run_dir).unwrap_err();
        assert!(err.contains("no events found"));
    }

    #[test]
    fn test_inspect_session_recovery_missing_events_file() {
        // arrange
        // act
        // assert
        let root = tempdir().unwrap();
        let run_dir = setup_session_dir(root.path(), "test-run");
        // Don't write events file

        let err = inspect_session_recovery(&run_dir).unwrap_err();
        assert!(err.contains("failed to read events file"));
    }

    #[test]
    fn test_inspect_session_recovery_not_resumable() {
        // arrange
        // act
        // assert
        let root = tempdir().unwrap();
        let sessions_dir = root.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let run_dir = setup_session_dir(&sessions_dir, "test-run");

        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt".into(), // prompt runs are not resumable
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                2,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ];
        write_events(&run_dir, &events);

        let summary = inspect_session_recovery(&run_dir).unwrap();
        assert_eq!(summary.run_id, "test-run");
        assert!(!summary.resumable);
        assert!(summary.resume_agent_id.is_none());
    }

    #[test]
    fn test_latest_run_name_empty() {
        // arrange
        // act
        // assert
        let events = vec![];
        assert_eq!(latest_run_name(&events), None);
    }

    #[test]
    fn test_latest_run_name_unrelated_events() {
        // arrange
        // act
        // assert
        let events = vec![envelope(
            1,
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )];
        assert_eq!(latest_run_name(&events), None);
    }

    #[test]
    fn test_latest_run_name_run_started() {
        // arrange
        // act
        // assert
        let events = vec![envelope(
            1,
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "my-run".into(),
                workspace_root: "/tmp".to_string(),
            }),
        )];
        assert_eq!(latest_run_name(&events), Some("my-run".to_string()));
    }

    #[test]
    fn test_latest_run_name_session_title_updated() {
        // arrange
        // act
        // assert
        let events = vec![envelope(
            1,
            None,
            EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                title: "my-title".to_string(),
            }),
        )];
        assert_eq!(latest_run_name(&events), Some("my-title".to_string()));
    }

    #[test]
    fn test_latest_run_name_multiple_events() {
        // arrange
        // act
        // assert
        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "my-run".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                2,
                None,
                EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                    title: "my-first-title".to_string(),
                }),
            ),
            envelope(
                3,
                None,
                EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                    title: "my-latest-title".to_string(),
                }),
            ),
        ];
        assert_eq!(
            latest_run_name(&events),
            Some("my-latest-title".to_string())
        );
    }

    #[test]
    fn test_latest_run_name_uses_last_matching_event_kind() {
        // arrange
        // act
        // assert
        let events = vec![
            envelope(
                1,
                None,
                EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                    title: "my-title".to_string(),
                }),
            ),
            envelope(
                2,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "later-run".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
        ];
        assert_eq!(latest_run_name(&events), Some("later-run".to_string()));
    }

    #[test]
    fn test_inspect_session_recovery_multiple_agents() {
        // arrange
        // act
        // assert
        let root = tempdir().unwrap();
        let sessions_dir = root.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        let run_dir = setup_session_dir(&sessions_dir, "test-run");

        let events = vec![
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                2,
                Some("agent-1"),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent-1".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                Some("agent-2"),
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent-2".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                4,
                Some("agent-1"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "hello from 1".to_string(),
                    request_digest: "digest1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                5,
                Some("agent-2"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_2".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "hello from 2".to_string(),
                    request_digest: "digest2".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                6,
                None,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ];
        write_events(&run_dir, &events);

        let summary = inspect_session_recovery(&run_dir).unwrap();
        assert_eq!(summary.resume_agent_id, Some("agent-2".to_string()));
    }
}
