use super::*;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(super) fn exact_test_key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[cfg(test)]
pub(super) fn exact_test_key_with_modifiers(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[cfg(test)]
pub(super) fn exact_test_session_entry(run_id: &str, run_dir: &str) -> app::SessionHistoryEntry {
    app::SessionHistoryEntry {
        run_dir: PathBuf::from(run_dir),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some("Resume target".to_string()),
            status: Some(harness_core::proj::RunStatus::Finished),
            last_updated_at: Some("2026-03-10T10:00:00Z".to_string()),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some("deep".to_string()),
            provider_model: Some("default/gpt-5.4-mini".to_string()),
            mode_source: harness_core::proj::SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    }
}

#[cfg(test)]
pub(crate) fn session_view_events() -> Vec<harness_core::event::EventEnvelopeV1> {
    vec![
        envelope(
            1,
            Some("req_001"),
            harness_core::event::EventV1::UserMessageSubmitted(
                harness_core::event::UserMessageSubmittedEvent {
                    request_id: "req_001".to_string(),
                    text: "Explain the refactor".to_string(),
                },
            ),
        ),
        envelope(
            2,
            Some("req_001"),
            harness_core::event::EventV1::ProviderRequestStarted(
                harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_001".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5-codex".to_string(),
                    prompt_summary: "Explain the refactor".to_string(),
                    request_digest: "digest-req-001".to_string(),
                    metadata: None,
                },
            ),
        ),
        envelope(
            3,
            Some("req_001"),
            harness_core::event::EventV1::ProviderStreamDelta(
                harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_001".to_string(),
                    delta: "Working through the steps.".to_string(),
                },
            ),
        ),
        envelope(
            4,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallRequested(
                harness_core::event::ToolCallRequestedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                    tool_id: "fs.read".to_string(),
                    args_summary: r#"{"path":"src/app.rs"}"#.to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                },
            ),
        ),
        permission_requested_event(5, "perm_1", "tool_call_1"),
        permission_resolved_event(6, "perm_1", harness_core::perm::PermissionDecision::Allow),
        envelope(
            7,
            Some("req_001"),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "tool_call_1".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("tool:fs.read".to_string()),
            }),
        ),
        envelope(
            8,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallStarted(
                harness_core::event::ToolCallStartedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                },
            ),
        ),
        envelope(
            9,
            Some("req_001"),
            harness_core::event::EventV1::ToolCallFinished(
                harness_core::event::ToolCallFinishedEvent {
                    tool_call_id: "tool_call_1".to_string(),
                    status: harness_core::event::ToolCallStatus::Succeeded,
                    output_summary: Some("tool output".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                    output_json: None,
                    metadata: None,
                },
            ),
        ),
        envelope(
            10,
            Some("req_001"),
            harness_core::event::EventV1::ProviderRequestFinished(
                harness_core::event::ProviderRequestFinishedEvent {
                    request_id: "req_001".to_string(),
                    finish_reason: "stop".to_string(),
                    output_digest: Some("digest-final".to_string()),
                    usage: None,
                    metadata: None,
                },
            ),
        ),
    ]
}

#[cfg(test)]
pub(super) fn orchestration_details_drawer_events(
    extra_terminal_rows: usize,
) -> Vec<EventEnvelopeV1> {
    let mut events = session_view_events();
    events.extend([
        envelope(
            11,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "w1".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            12,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "w2".to_string(),
                profile: "scout".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            13,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w1".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_stale".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some("scan".to_string()),
            }),
        ),
        envelope_with_actor(
            14,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w1".to_string()),
            ),
            harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
                task_id: "task_stale".to_string(),
                stale_for_ms: 3001,
            }),
        ),
        envelope_with_actor(
            15,
            Some("req_001"),
            harness_core::event::EventActor::new(harness_core::event::ActorKind::Supervisor, None),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_run".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: None,
            }),
        ),
        envelope_with_actor(
            16,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::System,
                Some("coordinator".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_queue".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("tool:read".to_string()),
            }),
        ),
        envelope_with_actor(
            17,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_done".to_string(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some("tool:done".to_string()),
            }),
        ),
        envelope_with_actor(
            18,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id: "task_done".to_string(),
                result_summary: "done".to_string(),
                result_digest: "digest-task-done".to_string(),
                metadata: None,
            }),
        ),
    ]);

    let mut seq = 19;
    for index in 0..extra_terminal_rows {
        let task_id = format!("task_tail_{index}");
        events.push(envelope_with_actor(
            seq,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: task_id.clone(),
                state: harness_core::event::TaskScheduleState::Started,
                queue_key: Some(format!("tail:{index}")),
            }),
        ));
        seq += 1;
        events.push(envelope_with_actor(
            seq,
            Some("req_001"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("w2".to_string()),
            ),
            harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id,
                result_summary: format!("tail {index} done"),
                result_digest: format!("digest-tail-{index}"),
                metadata: None,
            }),
        ));
        seq += 1;
    }

    events
}

#[cfg(test)]
pub(super) fn orchestration_details_drawer_app(extra_terminal_rows: usize) -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    for event in orchestration_details_drawer_events(extra_terminal_rows) {
        app.ingest_event(event);
    }
    app.handle_key(focus_cycle_key());
    app.handle_key(key(crossterm::event::KeyCode::Char('i')));
    app
}

#[cfg(test)]
pub(super) fn assert_session_view_state(app: &app::AppState) {
    assert_eq!(app.activities.len(), 1);

    let activity = app.activities.front().unwrap_or_abort();
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.provider_id, "openai");
    assert_eq!(activity.model_id, "gpt-5-codex");
    assert_eq!(activity.status, app::ActivityStatus::Done);
    assert_eq!(activity.thinking_text, "Working through the steps.");
    assert_eq!(activity.transcript_text, "");
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("Explain the refactor")
    );

    assert_eq!(activity.tool_calls.len(), 1);
    let tool_call = activity.tool_calls.first().unwrap_or_abort();
    assert_eq!(tool_call.tool_call_id, "tool_call_1");
    assert_eq!(tool_call.tool_id, "fs.read");
    assert_eq!(tool_call.status, app::ToolCallDisplayStatus::Succeeded);
    assert_eq!(tool_call.output_summary.as_deref(), Some("tool output"));
    assert_eq!(tool_call.truncated_output.as_deref(), Some("tool output"));

    assert!(app.active_permission().is_none());
}

#[cfg(test)]
pub(super) fn key(code: crossterm::event::KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

#[cfg(test)]
pub(super) fn focus_cycle_key() -> crossterm::event::KeyEvent {
    key_with_modifiers(
        crossterm::event::KeyCode::Tab,
        crossterm::event::KeyModifiers::CONTROL,
    )
}

#[cfg(test)]
pub(crate) fn key_with_modifiers(
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, modifiers)
}

#[cfg(test)]
pub(super) fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: permission_id.to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some(tool_call_id.to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    )
}

#[cfg(test)]
pub(super) fn permission_resolved_event(
    seq: u64,
    permission_id: &str,
    decision: harness_core::perm::PermissionDecision,
) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        Some("tool_call_1"),
        harness_core::event::EventV1::PermissionResolved(
            harness_core::event::PermissionResolvedEvent {
                permission_id: permission_id.to_string(),
                decision: match decision {
                    harness_core::perm::PermissionDecision::Allow => {
                        harness_core::event::PermissionDecision::Allow
                    }
                    harness_core::perm::PermissionDecision::Deny => {
                        harness_core::event::PermissionDecision::Deny
                    }
                },
                reason: Some("resolved in test".to_string()),
            },
        ),
    )
}

#[cfg(test)]
pub(super) fn startup_session_entry(
    run_id: &str,
    run_dir: &str,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    startup_session_entry_with_details(
        run_id,
        run_dir,
        &format!("run-{run_id}"),
        None,
        None,
        "default",
        "openai/gpt-5.4-mini",
        is_resumable,
        resume_disabled_reason,
    )
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps session-history fixture fields explicit at call sites"
)]
pub(super) fn startup_session_entry_with_details(
    run_id: &str,
    run_dir: &str,
    run_name: &str,
    status: Option<harness_core::proj::RunStatus>,
    last_updated_at: Option<&str>,
    profile_preset: &str,
    provider_model: &str,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    startup_session_entry_with_mode_and_details(
        run_id,
        run_dir,
        run_name,
        status,
        last_updated_at,
        profile_preset,
        provider_model,
        harness_core::proj::SessionModeSource::InteractiveLive,
        is_resumable,
        resume_disabled_reason,
    )
}

#[cfg(test)]
#[expect(
    clippy::too_many_arguments,
    reason = "test helper keeps session-history fixture fields explicit at call sites"
)]
pub(super) fn startup_session_entry_with_mode_and_details(
    run_id: &str,
    run_dir: &str,
    run_name: &str,
    status: Option<harness_core::proj::RunStatus>,
    last_updated_at: Option<&str>,
    profile_preset: &str,
    provider_model: &str,
    mode_source: harness_core::proj::SessionModeSource,
    is_resumable: bool,
    resume_disabled_reason: Option<&str>,
) -> app::SessionHistoryEntry {
    app::SessionHistoryEntry {
        run_dir: PathBuf::from(run_dir),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: run_id.to_string(),
            run_name: Some(run_name.to_string()),
            status,
            last_updated_at: last_updated_at.map(str::to_string),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some(profile_preset.to_string()),
            provider_model: Some(provider_model.to_string()),
            mode_source,
            is_resumable,
            resume_disabled_reason: resume_disabled_reason.map(str::to_string),
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    }
}

#[cfg(test)]
pub(super) fn test_timestamp_days_ago(days_ago: i64, time_hh_mm: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_abort();
    let today_days = i64::try_from(now.as_secs() / 86_400).unwrap_or_abort();
    let date = test_civil_date_from_days_since_epoch(today_days - days_ago);
    format!("{date}T{time_hh_mm}:00Z")
}

#[cfg(test)]
pub(super) fn test_civil_date_from_days_since_epoch(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
pub(super) fn envelope(
    seq: u64,
    correlation_id: Option<&str>,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    envelope_with_actor(
        seq,
        correlation_id,
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("coordinator".to_string()),
        ),
        payload,
    )
}

#[cfg(test)]
pub(super) fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: harness_core::event::EventActor,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

#[cfg(test)]
pub(super) fn orchestration_status_strip_fixture() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_alpha".to_string(),
            profile: "researcher".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        None,
        harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
            agent_id: "agent_beta".to_string(),
            profile: "reviewer".to_string(),
            parent_agent_id: None,
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_orch_queued"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_queued".to_string(),
            state: harness_core::event::TaskScheduleState::Queued,
            queue_key: Some("agent:queued:alpha".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        4,
        Some("req_orch_running"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_beta".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_running".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:beta".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        5,
        Some("req_orch_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_stale".to_string(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("agent:running:alpha".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        6,
        Some("req_orch_stale"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_alpha".to_string()),
        ),
        harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
            task_id: "task_stale".to_string(),
            stale_for_ms: 3001,
        }),
    ));

    app
}

#[cfg(test)]
pub(super) fn orchestration_details_drawer_card_body(
    app: &app::AppState,
    height: u16,
    width: u16,
) -> String {
    ui::orchestration_card_text_for_test(app, height, width).join("\n")
}

#[cfg(test)]
pub(super) fn operator_sidebar_text(app: &app::AppState) -> String {
    ui::operator_sidebar_text_for_test(app).join("\n")
}

#[cfg(test)]
pub(super) fn operator_sidebar_edit_only_event(seq: u64) -> harness_core::event::EventEnvelopeV1 {
    envelope(
        seq,
        None,
        harness_core::event::EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: format!("edit_{seq}"),
            path: "src/ui_secondary.rs".to_string(),
            new_file_digest: format!("digest-edit-{seq}"),
            diff_rel_path: Some(format!("artifacts/edit-{seq}.diff")),
            diff_digest: Some(format!("digest-edit-artifact-{seq}")),
        }),
    )
}

#[cfg(test)]
pub(super) fn operator_sidebar_empty_live_app() -> app::AppState {
    app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    )
}

#[cfg(test)]
pub(super) fn operator_sidebar_todo_live_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    app
}

#[cfg(test)]
pub(super) fn operator_sidebar_modified_files_live_app() -> app::AppState {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(operator_sidebar_edit_only_event(1));
    app
}

#[cfg(test)]
pub(super) fn operator_sidebar_todo_replay_app() -> app::AppState {
    app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events())
}

#[cfg(test)]
pub(super) fn operator_sidebar_modified_files_replay_app() -> app::AppState {
    app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![operator_sidebar_edit_only_event(1)],
    )
}

#[cfg(test)]
pub(super) fn operator_sidebar_child_navigation_replay_app() -> app::AppState {
    let mut events = session_view_events();
    let metadata = harness_core::event::ToolCallMetadata {
        canonical_tool_id: Some("task".to_string()),
        lineage: Some(harness_core::event::TaskLineageMetadata {
            parent_session_id: Some("parent_run".to_string()),
            child_session_id: Some("child_run".to_string()),
            ..harness_core::event::TaskLineageMetadata::default()
        }),
        artifact_refs: vec![harness_core::event::EventArtifactRef {
            path: "artifacts/toolcalls/task/result.json".to_string(),
            digest: Some("digest-task-artifact".to_string()),
        }],
        ..harness_core::event::ToolCallMetadata::default()
    };
    events.push(envelope(
        11,
        Some("req_001"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tool_call_child_nav".to_string(),
                tool_id: "task".to_string(),
                args_summary: r#"{"title":"inspect child session"}"#.to_string(),
                args_digest: "digest-tool-child-nav".to_string(),
                metadata: Some(metadata.clone()),
            },
        ),
    ));
    events.push(envelope(
        12,
        Some("req_001"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tool_call_child_nav".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("child session recorded".to_string()),
                output_digest: Some("digest-tool-child-nav-output".to_string()),
                output_json: None,
                metadata: Some(metadata),
            },
        ),
    ));
    app::AppState::new_replay(PathBuf::from("/tmp/child_run"), events)
}
