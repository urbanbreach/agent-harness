//! Headless session recovery helpers shared by CLI entrypoints.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use harness_core::event::{ActorKind, EventEnvelopeV1, EventV1};
use harness_core::proj::{
    inspect_resume_plan, project_run_summary, ResumePlan, RunStatus, SessionModeSource,
};
use harness_tui::load_events_from_run_dir;
use serde::Serialize;

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
                "harness prompt --resume {} --text \"<next prompt>\"",
                catalog.run_id
            )
        }),
    })
}

pub fn latest_run_name(events: &[EventEnvelopeV1]) -> Option<String> {
    events.iter().rev().find_map(|event| {
        if let EventV1::RunStarted(data) = &event.payload {
            Some(data.run_name.clone())
        } else {
            None
        }
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
    if !run_dir.join("events.jsonl").exists() {
        return Err(format!(
            "{} does not contain an events.jsonl session log",
            run_dir.display()
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
                request_id: Some(payload.request_id.clone()),
                agent_id: None,
                text: payload.text.clone(),
            }),
            EventV1::ProviderRequestStarted(payload) => Some(RecoveryPromptContextEntry {
                seq: event.seq,
                kind: "provider_prompt".to_string(),
                request_id: Some(payload.request_id.clone()),
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
            provider_model: match (child.provider_id.as_deref(), child.model_id.as_deref()) {
                (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
                (Some(provider), None) => Some(format!("{provider}/<unavailable>")),
                (None, Some(model)) => Some(format!("<unavailable>/{model}")),
                (None, None) => None,
            },
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
            tool_call_id: artifact.tool_call_id.clone(),
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

fn most_recent_conversational_agent_id(
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
