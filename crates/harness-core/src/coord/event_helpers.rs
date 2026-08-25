// allow: SIZE_OK — coordinator state machine (turn lifecycle + scheduling)
use std::{collections::BTreeMap, fs};

use crate::clock::Clock;
use crate::digest::digest12;
use crate::event::{
    ActorKind, ArtifactWrittenEvent, EditAppliedEvent, EditProposedEvent, EditRejectedEvent,
    EventActor, EventBuilder, EventContext, EventEnvelopeV1, EventV1, HookExecutionMetadata,
    LiveEventContext, LiveEventV1, PermissionDecision as EventPermissionDecision,
    PermissionGrantRecordedEvent, PermissionRequestedArgs, PermissionResolvedEvent,
    ToolCallFinishedEvent, ToolCallMetadata, ToolCallStartedEvent, ToolCallStatus,
    ToolIdentityMetadata,
};
use crate::perm::PermissionGrant;
use crate::redact::Redactor;
use crate::store::{EventEnvelopeWithoutSeqV1, EventStore};
use crate::tool::ArtifactRef;

use super::{
    failed_tool_output_json, mirror_event_to_child_session, CoordinatorError, EditAppliedEventArgs,
    HashlineEditMetadata, PermissionRequestedEventArgs, RunState, ToolCallFinishedEventArgs,
    ToolCallRequestedEventArgs, COORDINATOR_AGENT_ID,
};

pub(in crate::coord) fn append_permission_resolved_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    permission_id: String,
    decision: EventPermissionDecision,
    reason: Option<String>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_payload_event(
        clock,
        redactor,
        run_state,
        system_actor(),
        Some(format!("permission:{permission_id}")),
        EventV1::PermissionResolved(PermissionResolvedEvent {
            permission_id,
            decision,
            reason,
        }),
    )
}

pub(in crate::coord) fn append_payload_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    actor: EventActor,
    stream_key: Option<String>,
    payload: EventV1,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_payload_event_with_correlation(
        clock, redactor, run_state, actor, stream_key, None, payload,
    )
}

pub(in crate::coord) fn append_payload_event_with_correlation<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    actor: EventActor,
    stream_key: Option<String>,
    correlation_id: Option<String>,
    payload: EventV1,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = event_context_with_keys(run_state, actor, stream_key, correlation_id);
    let envelope = builder.build(context, payload)?;
    append_built_event(run_state, envelope)
}

pub(in crate::coord) struct LiveEventPublishArgs {
    pub(in crate::coord) actor: EventActor,
    pub(in crate::coord) stream_key: Option<String>,
    pub(in crate::coord) correlation_id: Option<String>,
    pub(in crate::coord) payload: LiveEventV1,
}

pub(in crate::coord) fn publish_live_event<C, R>(
    builder: &EventBuilder<'_, C, R>,
    run_state: &mut RunState,
    args: LiveEventPublishArgs,
) -> Result<(), CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let LiveEventPublishArgs {
        actor,
        stream_key,
        correlation_id,
        payload,
    } = args;
    let mut context = LiveEventContext::new(
        format!("live_evt-{:020}", run_state.next_live_event_id),
        actor,
    );
    context.correlation_id = correlation_id;
    context.stream_key = stream_key;
    let envelope = builder.build_live(context, payload)?;
    run_state.next_live_event_id += 1;
    run_state.event_store.publish_live(envelope);
    Ok(())
}

fn event_context_with_keys(
    run_state: &RunState,
    actor: EventActor,
    stream_key: Option<String>,
    correlation_id: Option<String>,
) -> EventContext {
    let mut context = EventContext::new(run_state.next_event_seq, actor);
    context.correlation_id = correlation_id;
    context.stream_key = stream_key;
    context
}

fn event_context_with_correlation_fallback(
    run_state: &RunState,
    actor: EventActor,
    stream_key: String,
    request_correlation_id: Option<&str>,
    fallback_correlation_id: &str,
) -> EventContext {
    event_context_with_keys(
        run_state,
        actor,
        Some(stream_key),
        Some(
            request_correlation_id
                .unwrap_or(fallback_correlation_id)
                .to_string(),
        ),
    )
}

fn tool_call_event_context(
    run_state: &RunState,
    actor: EventActor,
    tool_call_id: &str,
    request_correlation_id: Option<&str>,
) -> EventContext {
    event_context_with_correlation_fallback(
        run_state,
        actor,
        format!("tool_call:{tool_call_id}"),
        request_correlation_id,
        tool_call_id,
    )
}

fn permission_event_context(
    run_state: &RunState,
    permission_id: &str,
    request_correlation_id: Option<&str>,
    fallback_correlation_id: &str,
) -> EventContext {
    event_context_with_correlation_fallback(
        run_state,
        system_actor(),
        format!("permission:{permission_id}"),
        request_correlation_id,
        fallback_correlation_id,
    )
}

fn edit_event_context(
    run_state: &RunState,
    edit_id: &str,
    tool_call_id: &str,
    request_correlation_id: Option<&str>,
) -> EventContext {
    event_context_with_correlation_fallback(
        run_state,
        system_actor(),
        format!("edit:{edit_id}"),
        request_correlation_id,
        tool_call_id,
    )
}

pub(in crate::coord) fn append_tool_call_requested_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: ToolCallRequestedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallRequestedEventArgs {
        actor,
        tool_call_id,
        tool_id,
        args_json,
        tool_metadata,
        request_correlation_id,
    } = args;

    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = tool_call_event_context(run_state, actor, tool_call_id, request_correlation_id);
    let envelope =
        builder.tool_call_requested(context, tool_call_id, tool_id, args_json, tool_metadata)?;
    let appended = append_built_event(run_state, envelope)?;
    run_state
        .tool_call_request_event_ids
        .insert(tool_call_id.to_string(), appended.event_id.clone());
    Ok(appended)
}

pub(in crate::coord) fn append_permission_requested_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: PermissionRequestedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let PermissionRequestedEventArgs {
        permission_id,
        tool_call_id,
        kind,
        summary,
        request_digest,
        timeout_ms,
        default_decision,
        request_correlation_id,
    } = args;
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = permission_event_context(
        run_state,
        permission_id,
        request_correlation_id,
        tool_call_id,
    );

    let envelope = builder.permission_requested(
        context,
        PermissionRequestedArgs {
            permission_id: permission_id.to_string(),
            kind: kind.as_str().to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary,
            request_digest,
            timeout_ms,
            default_decision,
        },
    )?;

    append_built_event(run_state, envelope)
}

pub(in crate::coord) fn append_permission_grant_recorded_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    permission_id: &str,
    request_correlation_id: Option<&str>,
    grant: PermissionGrant,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = permission_event_context(
        run_state,
        permission_id,
        request_correlation_id,
        permission_id,
    );
    let envelope = builder.build(
        context,
        EventV1::PermissionGrantRecorded(PermissionGrantRecordedEvent { grant }),
    )?;
    append_built_event(run_state, envelope)
}

pub(in crate::coord) fn append_tool_call_started_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = tool_call_event_context(
        run_state,
        system_actor(),
        tool_call_id,
        request_correlation_id,
    );
    let envelope = builder.build(
        context,
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.into(),
        }),
    )?;
    append_built_event(run_state, envelope)
}

pub(in crate::coord) fn append_tool_call_finished_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: ToolCallFinishedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let ToolCallFinishedEventArgs {
        tool_call_id,
        status,
        output_summary,
        output_json,
        metadata,
        request_correlation_id,
        causation_id,
    } = args;
    let output_digest = output_summary.as_ref().map(|s| digest12(s.as_bytes()));
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let mut context = tool_call_event_context(
        run_state,
        system_actor(),
        tool_call_id,
        request_correlation_id,
    );
    context.causation_id = causation_id.map(str::to_string);
    let envelope = builder.build(
        context,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status,
            output_summary,
            output_digest,
            output_json,
            metadata,
        }),
    )?;
    let appended = append_built_event(run_state, envelope)?;
    run_state.tool_call_request_event_ids.remove(tool_call_id);
    Ok(appended)
}

pub(in crate::coord) fn append_edit_proposed_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    metadata: &HashlineEditMetadata,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = edit_event_context(
        run_state,
        &metadata.edit_id,
        tool_call_id,
        request_correlation_id,
    );

    let envelope = builder.build(
        context,
        EventV1::EditProposed(EditProposedEvent {
            edit_id: metadata.edit_id.clone(),
            path: metadata.path.clone(),
            summary: metadata.summary.clone(),
            patch_digest: metadata.patch_digest.clone(),
        }),
    )?;

    append_built_event(run_state, envelope)
}

pub(in crate::coord) fn append_edit_applied_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    args: EditAppliedEventArgs<'_>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let EditAppliedEventArgs {
        tool_call_id,
        metadata,
        new_file_digest,
        diff_rel_path,
        diff_digest,
        request_correlation_id,
    } = args;
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = edit_event_context(
        run_state,
        &metadata.edit_id,
        tool_call_id,
        request_correlation_id,
    );

    let envelope = builder.build(
        context,
        EventV1::EditApplied(EditAppliedEvent {
            edit_id: metadata.edit_id.clone(),
            path: metadata.path.clone(),
            new_file_digest,
            diff_rel_path,
            diff_digest,
        }),
    )?;

    append_built_event(run_state, envelope)
}

pub(in crate::coord) fn append_edit_rejected_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    metadata: &HashlineEditMetadata,
    reason: String,
    request_correlation_id: Option<&str>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = edit_event_context(
        run_state,
        &metadata.edit_id,
        tool_call_id,
        request_correlation_id,
    );

    let envelope = builder.build(
        context,
        EventV1::EditRejected(EditRejectedEvent {
            edit_id: metadata.edit_id.clone(),
            path: metadata.path.clone(),
            reason,
        }),
    )?;

    append_built_event(run_state, envelope)
}

#[expect(
    clippy::too_many_arguments,
    reason = "failed tool-call terminal events carry explicit metadata and hook context"
)]
pub(in crate::coord) fn append_failed_tool_call_finished_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    reason: &str,
    request_correlation_id: Option<&str>,
    metadata: Option<ToolCallMetadata>,
    hook_executions: &[HookExecutionMetadata],
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_tool_call_finished_event(
        clock,
        redactor,
        run_state,
        ToolCallFinishedEventArgs {
            tool_call_id,
            status: ToolCallStatus::Failed,
            output_summary: Some(reason.to_string()),
            output_json: Some(failed_tool_output_json(reason, hook_executions)),
            metadata,
            request_correlation_id,
            causation_id: None,
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "pre-start failed terminals carry exact request causation and hook context"
)]
pub(in crate::coord) fn append_prestart_failed_tool_call_finished_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    reason: &str,
    request_correlation_id: Option<&str>,
    request_event_id: &str,
    metadata: Option<ToolCallMetadata>,
    hook_executions: &[HookExecutionMetadata],
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    append_tool_call_finished_event(
        clock,
        redactor,
        run_state,
        ToolCallFinishedEventArgs {
            tool_call_id,
            status: ToolCallStatus::Failed,
            output_summary: Some(reason.to_string()),
            output_json: Some(failed_tool_output_json(reason, hook_executions)),
            metadata,
            request_correlation_id,
            causation_id: Some(request_event_id),
        },
    )
}

pub(in crate::coord) fn append_artifact_written_event<C, R>(
    clock: &C,
    redactor: &R,
    run_state: &mut RunState,
    tool_call_id: &str,
    artifact: &ArtifactRef,
    request_correlation_id: Option<&str>,
    tool_metadata: Option<&ToolIdentityMetadata>,
) -> Result<EventEnvelopeV1, CoordinatorError>
where
    C: Clock + ?Sized,
    R: Redactor + ?Sized,
{
    let artifact_path = run_state.info.run_dir.join(&artifact.path);
    let bytes = fs::metadata(&artifact_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    let digest = artifact
        .digest
        .clone()
        .unwrap_or_else(|| digest12(artifact.path.as_bytes()));
    let mut metadata = BTreeMap::new();
    metadata.insert("tool_call_id".to_string(), tool_call_id.to_string());
    if let Some(tool_metadata) = tool_metadata {
        if let Some(canonical_tool_id) = tool_metadata.canonical_tool_id.as_ref() {
            metadata.insert("canonical_tool_id".to_string(), canonical_tool_id.clone());
        }
        if let Some(alias_source_tool_id) = tool_metadata.alias_source_tool_id.as_ref() {
            metadata.insert(
                "alias_source_tool_id".to_string(),
                alias_source_tool_id.clone(),
            );
        }
    }

    let builder = EventBuilder::new(clock, redactor, run_state.info.run_id.to_string());
    let context = tool_call_event_context(
        run_state,
        system_actor(),
        tool_call_id,
        request_correlation_id,
    );
    let envelope = builder.build(
        context,
        EventV1::ArtifactWritten(ArtifactWrittenEvent {
            path: artifact.path.clone(),
            digest,
            bytes,
            tool_call_id: Some(tool_call_id.into()),
            tool_metadata: tool_metadata.cloned(),
            metadata,
        }),
    )?;

    append_built_event(run_state, envelope)
}

fn append_built_event(
    run_state: &mut RunState,
    envelope: EventEnvelopeV1,
) -> Result<EventEnvelopeV1, CoordinatorError> {
    let expected_seq = run_state.next_event_seq;
    let appended = run_state
        .event_store
        .append(EventEnvelopeWithoutSeqV1::from(envelope))?;

    if appended.seq != expected_seq {
        return Err(CoordinatorError::EventSequenceMismatch {
            expected: expected_seq,
            actual: appended.seq,
        });
    }

    run_state.next_event_seq += 1;
    mirror_event_to_child_session(run_state, &appended)?;
    Ok(appended)
}

pub(in crate::coord) fn system_actor() -> EventActor {
    EventActor::new(ActorKind::System, Some(COORDINATOR_AGENT_ID.to_string()))
}

pub(in crate::coord) fn agent_actor(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Worker, Some(agent_id.to_string()))
}
