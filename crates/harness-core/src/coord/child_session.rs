// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::fs;
use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use crate::clock::Clock;
use crate::event::{
    AgentSpawnedEvent, EventActor, EventBuilder, EventContext, EventEnvelopeV1, EventV1,
    RunFinishedEvent, RunStartedEvent,
};
use crate::redact::Redactor;
use crate::session::canonical_provider_fragment_for_event;
use crate::session_paths::{EVENTS_FILE_NAME, META_FILE_NAME};
use crate::session_title::create_default_title;
use crate::store::{EventEnvelopeWithoutSeqV1, EventStore, JsonlFileEventStore};
use crate::text::non_empty_trimmed;

use super::{system_actor, CoordinatorConfig, CoordinatorError, RunState};

#[derive(Debug)]
pub(super) struct ChildSessionMirror {
    pub(super) event_store: Arc<JsonlFileEventStore>,
    pub(super) append_parent_finish: bool,
}

pub(super) fn create_child_session_mirror<C, R>(
    clock: &C,
    redactor: &R,
    config: &CoordinatorConfig,
    run_state: &mut RunState,
    child_session_id: &str,
    profile: &str,
    child_session_title: Option<&str>,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    if run_state
        .child_session_mirrors
        .contains_key(child_session_id)
    {
        return Ok(());
    }

    let event_store = Arc::new(config.event_store_opener.open(
        &config.session_dir,
        child_session_id,
        config.deterministic_store,
    )?);
    let run_dir = config.session_dir.join(child_session_id);
    let title = child_session_title
        .and_then(non_empty_trimmed)
        .map(str::to_string)
        .unwrap_or_else(|| create_default_title(clock, true));

    write_child_session_metadata(
        clock,
        config,
        run_state,
        child_session_id,
        &run_dir,
        &title,
        profile,
    )?;

    let child_appender = ChildPayloadAppender {
        clock,
        redactor,
        event_store: event_store.as_ref(),
        child_run_id: child_session_id,
    };
    child_appender.append(
        system_actor(),
        Some(format!("run:{child_session_id}")),
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: title.into(),
            workspace_root: run_state.info.workspace_root.display().to_string(),
        }),
    )?;
    child_appender.append(
        system_actor(),
        Some(format!("agent:{child_session_id}")),
        None,
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: child_session_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: None,
        }),
    )?;

    run_state.child_session_mirrors.insert(
        child_session_id.to_string(),
        ChildSessionMirror {
            event_store,
            append_parent_finish: true,
        },
    );
    Ok(())
}

pub(super) fn restore_child_session_mirrors<C, R>(
    clock: &C,
    redactor: &R,
    config: &CoordinatorConfig,
    run_state: &mut RunState,
    restored_agent_bindings: &[(String, String, Option<String>)],
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    for (agent_id, profile, parent_agent_id) in restored_agent_bindings {
        if parent_agent_id.is_none() {
            continue;
        }

        let run_dir = config.session_dir.join(agent_id);
        if run_dir.join(EVENTS_FILE_NAME).exists() {
            let event_store = Arc::new(config.event_store_opener.open_existing(
                &config.session_dir,
                agent_id,
                config.deterministic_store,
            )?);
            run_state.child_session_mirrors.insert(
                agent_id.clone(),
                ChildSessionMirror {
                    event_store,
                    append_parent_finish: false,
                },
            );
        } else {
            create_child_session_mirror(
                clock, redactor, config, run_state, agent_id, profile, None,
            )?;
        }
    }

    Ok(())
}

fn write_child_session_metadata<C>(
    clock: &C,
    config: &CoordinatorConfig,
    run_state: &RunState,
    child_session_id: &str,
    child_run_dir: &Path,
    title: &str,
    profile: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
{
    let created_at = if config.deterministic_store {
        None
    } else {
        clock.system_time_rfc3339()
    };
    let metadata = json!({
        "run_id": child_session_id,
        "run_name": title,
        "workspace_root": run_state.info.workspace_root.display().to_string(),
        "created_at": created_at,
        "config_digest": config.config_digest.clone(),
        "harness_version": config.harness_version.clone(),
        "recorded_runtime_context": null,
        "harness_lineage": {
            "relationship": "task_child_session",
            "parent_run_id": run_state.info.run_id.to_string(),
            "parent_session_id": run_state.info.run_id.to_string(),
            "child_session_id": child_session_id,
            "profile": profile,
        }
    });
    let meta_path = child_run_dir.join(META_FILE_NAME);
    let body = serde_json::to_string_pretty(&metadata)?;
    fs::write(&meta_path, body).map_err(|source| CoordinatorError::WriteRunMetadata {
        path: meta_path.display().to_string(),
        source,
    })
}

struct ChildPayloadAppender<'a, C, R>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    clock: &'a C,
    redactor: &'a R,
    event_store: &'a JsonlFileEventStore,
    child_run_id: &'a str,
}

impl<C, R> ChildPayloadAppender<'_, C, R>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    fn append(
        &self,
        actor: EventActor,
        stream_key: Option<String>,
        correlation_id: Option<String>,
        payload: EventV1,
    ) -> Result<EventEnvelopeV1, CoordinatorError> {
        let builder = EventBuilder::new(self.clock, self.redactor, self.child_run_id.to_string());
        let mut context = EventContext::new(self.event_store.next_seq()?, actor);
        context.stream_key = stream_key;
        context.correlation_id = correlation_id;
        let envelope = builder.build(context, payload)?;
        Ok(self
            .event_store
            .append(EventEnvelopeWithoutSeqV1::from(envelope))?)
    }
}

pub(super) fn mirror_event_to_child_session(
    run_state: &mut RunState,
    event: &EventEnvelopeV1,
) -> Result<(), CoordinatorError> {
    let Some(child_session_id) = child_session_id_for_event(run_state, event) else {
        return Ok(());
    };
    let Some(mirror) = run_state.child_session_mirrors.get(&child_session_id) else {
        return Ok(());
    };

    let mut child_event = event.clone();
    child_event.run_id = child_session_id.clone().into();
    child_event.seq = mirror.event_store.next_seq()?;
    child_event.event_id = format!("evt_{child_session_id}_mirror_{:012}", event.seq);
    if child_event.stream_key.as_deref() == Some(format!("run:{}", run_state.info.run_id).as_str())
    {
        child_event.stream_key = Some(format!("run:{child_session_id}"));
    }

    mirror
        .event_store
        .append(EventEnvelopeWithoutSeqV1::from(child_event))?;
    Ok(())
}

fn child_session_id_for_event(run_state: &RunState, event: &EventEnvelopeV1) -> Option<String> {
    if matches!(
        event.payload,
        EventV1::RunStarted(_) | EventV1::RunFinished(_)
    ) {
        return None;
    }

    if let Some(agent_id) = event.actor.agent_id.as_deref() {
        if run_state.child_session_mirrors.contains_key(agent_id) {
            return Some(agent_id.to_string());
        }
    }

    if let Some(request_id) = event.correlation_id.as_deref() {
        if let Some(child_session_id) = run_state.child_request_session_by_id.get(request_id) {
            return Some(child_session_id.clone());
        }
    }

    if let Some(fragment) = canonical_provider_fragment_for_event(event) {
        return run_state
            .child_request_session_by_id
            .get(fragment.request_id)
            .cloned();
    }

    match &event.payload {
        EventV1::ProviderRequestStarted(payload) => run_state
            .child_request_session_by_id
            .get(payload.request_id.as_str())
            .cloned(),
        EventV1::ProviderRequestFinished(payload) => run_state
            .child_request_session_by_id
            .get(payload.request_id.as_str())
            .cloned(),
        EventV1::AssistantMessageFinished(payload) => run_state
            .child_request_session_by_id
            .get(payload.request_id.as_str())
            .cloned(),
        _ => None,
    }
}

pub(super) fn finish_child_session_mirrors<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &RunState,
    summary: &str,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    for (child_session_id, mirror) in &run_state.child_session_mirrors {
        if !mirror.append_parent_finish {
            continue;
        }
        let child_appender = ChildPayloadAppender {
            clock,
            redactor,
            event_store: mirror.event_store.as_ref(),
            child_run_id: child_session_id,
        };
        child_appender.append(
            system_actor(),
            Some(format!("run:{child_session_id}")),
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: format!("parent session finished: {summary}"),
            }),
        )?;
    }

    Ok(())
}

// Branch summarization integration point:
//
// When a child session finishes and the user navigates back to the parent,
// `Coordinator::summarize_session_branch` should be called to generate a
// `BranchSummary` event for the abandoned branch. This is currently triggered
// by the TUI/CLI session-switching path rather than here, because
// `finish_child_session_mirrors` is a free function without provider access.
//
// To wire it here, pass `Arc<dyn Provider>` and `CompactionSettings` into this
// function, then call `summarize_session_branch` for each child session's
// agent before appending the `RunFinished` event above.
