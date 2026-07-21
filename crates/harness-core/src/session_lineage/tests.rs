use crate::UnwrapOrAbort;
use std::fs;
use std::path::{Path, PathBuf};

use super::{
    latest_clone_stable_prefix, materialize_child_session_inner, project_lineage_tree,
    validate_fork_stable_prefix, validate_stable_prefix, validate_tui_fork_stable_prefix,
    ChildRunIdSource, ChildSessionMaterializationError, ChildSessionMaterializationRequest,
    ChildSessionMaterializationSourceKind, SessionLineageError, SystemChildRunIdSource,
};
use crate::event::{
    ActorKind, AgentSpawnedEvent, EditAppliedEvent, EditProposedEvent, EventActor, EventEnvelopeV1,
    EventV1, PermissionDecision, PermissionRequestedEvent, PermissionResolvedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderRequestStartedMetadata,
    RunFinishedEvent, RunStartedEvent, TaskScheduleState, TaskScheduledEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use crate::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use crate::session_paths::EVENTS_FILE_NAME;

struct StaticChildRunIdSource(&'static str);

impl ChildRunIdSource for StaticChildRunIdSource {
    fn next_child_run_id(&self) -> String {
        self.0.to_string()
    }
}

#[test]
fn session_lineage_projects_tree_root_child_sibling_deep_ordering() {
    // arrange
    // act
    // assert
    let tree = project_lineage_tree(vec![
        entry("child-old", Some("root"), "2026-05-03T00:01:00Z"),
        entry("grandchild", Some("child-new"), "2026-05-03T00:03:00Z"),
        entry("root", None, "2026-05-03T00:00:00Z"),
        entry("child-new", Some("root"), "2026-05-03T00:02:00Z"),
    ]);

    let flattened = tree
        .flatten()
        .into_iter()
        .map(|row| (row.depth, row.entry.run_id.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(tree.len(), 4);
    assert_eq!(
        flattened,
        vec![
            (0, "root"),
            (1, "child-new"),
            (2, "grandchild"),
            (1, "child-old"),
        ]
    );
}

#[test]
fn session_lineage_handles_empty_sessions() {
    // arrange
    // act
    // assert
    let selected = validate_stable_prefix(&[], 0).unwrap_or_abort();
    let latest = latest_clone_stable_prefix(&[]).unwrap_or_abort();
    let tree = project_lineage_tree(Vec::new());

    assert_eq!(selected.cutoff_seq, 0);
    assert_eq!(selected.event_count, 0);
    assert_eq!(latest, selected);
    assert!(tree.is_empty());
}

#[test]
fn session_lineage_accepts_stable_prefix() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            3,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "prompt".to_string(),
                request_digest: "digest-req".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            5,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
        envelope(
            6,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "resumed".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
    ];

    let fork = validate_fork_stable_prefix(&events, 5).unwrap_or_abort();
    let latest = latest_clone_stable_prefix(&events).unwrap_or_abort();

    assert_eq!(fork.cutoff_seq, 5);
    assert_eq!(fork.event_count, 5);
    assert_eq!(fork.run_id.as_deref(), Some("run_session_lineage"));
    assert_eq!(fork.status, Some(RunStatus::Finished));
    assert_eq!(latest.cutoff_seq, 5);
}

#[test]
fn session_lineage_clears_user_prompt_by_provider_turn_metadata() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_turn".into(),
                text: "do work".to_string(),
            }),
        ),
        envelope(
            3,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider_req".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "do work".to_string(),
                request_digest: "digest-req".to_string(),
                metadata: Some(ProviderRequestStartedMetadata {
                    turn_id: Some("req_turn".to_string()),
                    provider_call_id: Some("provider_req".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        envelope(
            4,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider_req".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            5,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let prefix = validate_stable_prefix(&events, 5).unwrap_or_abort();
    assert_eq!(prefix.cutoff_seq, 5);
}

#[test]
fn session_lineage_treats_background_wakeup_message_as_delivered() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_background_wakeup".into(),
                text: "<system-reminder>\n[BACKGROUND TASK COMPLETED]\nID: agent_child\n</system-reminder>"
                    .to_string(),
            }),
        ),
        envelope(
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let prefix = validate_stable_prefix(&events, 3).unwrap_or_abort();
    assert_eq!(prefix.cutoff_seq, 3);
}

#[test]
fn session_lineage_rejects_in_flight_prefix() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:default:gpt-5".to_string()),
            }),
        ),
        envelope(
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "terminal but task remained open".to_string(),
            }),
        ),
    ];

    let err = validate_fork_stable_prefix(&events, 3).expect_err("task remains in flight");

    assert!(matches!(
        err,
        SessionLineageError::UnstablePrefix {
            cutoff_seq: 3,
            ref reason
        } if reason.contains("tasks are still in flight")
            && reason.contains("task_000001")
    ));
}

#[test]
fn session_lineage_tui_accepts_live_message_snapshot_with_unanswered_prompt() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_unanswered".into(),
                text: "Unanswered prompt".to_string(),
            }),
        ),
    ];

    let live_prefix = validate_tui_fork_stable_prefix(&events, 3).unwrap_or_abort();
    assert_eq!(live_prefix.cutoff_seq, 3);
    assert_eq!(live_prefix.event_count, 3);

    let prompt_row_prefix = validate_tui_fork_stable_prefix(&events, 2).unwrap_or_abort();
    assert_eq!(prompt_row_prefix.cutoff_seq, 2);
    assert_eq!(prompt_row_prefix.event_count, 2);
}

#[test]
fn session_lineage_tui_closes_historical_native_edit_id_mismatch_by_path() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            3,
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit-tool_1".to_string(),
                path: "demo.txt".to_string(),
                summary: "rewrite file through native edit tool".to_string(),
                patch_digest: "digest-native-edit".to_string(),
            }),
        ),
        envelope(
            4,
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "create-demo".to_string(),
                path: "demo.txt".to_string(),
                new_file_digest: "digest-demo".to_string(),
                diff_rel_path: Some("artifacts/toolcalls/edit-create-demo.diff".to_string()),
                diff_digest: Some("digest-demo-diff".to_string()),
            }),
        ),
    ];

    let prefix = validate_tui_fork_stable_prefix(&events, 4).unwrap_or_abort();

    assert_eq!(prefix.cutoff_seq, 4);
    assert_eq!(prefix.event_count, 4);
}

#[test]
fn session_lineage_tui_accepts_live_snapshot_with_unfinished_native_edit() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit-tool_1".to_string(),
                path: "demo.txt".to_string(),
                summary: "rewrite file through native edit tool".to_string(),
                patch_digest: "digest-native-edit".to_string(),
            }),
        ),
    ];

    let prefix = validate_tui_fork_stable_prefix(&events, 2).unwrap_or_abort();

    assert_eq!(prefix.cutoff_seq, 2);
    assert_eq!(prefix.event_count, 2);
}

#[test]
fn session_lineage_rejects_corrupt_non_contiguous_logs() {
    // arrange
    // act
    // assert
    let non_contiguous = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];
    assert!(matches!(
        validate_stable_prefix(&non_contiguous, 1),
        Err(SessionLineageError::NonContiguousSeq {
            expected: 2,
            actual: 3
        })
    ));

    let mut wrong_schema = vec![envelope(
        1,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "finished".to_string(),
        }),
    )];
    wrong_schema[0].schema_version = SCHEMA_VERSION + 1;
    assert!(matches!(
        validate_stable_prefix(&wrong_schema, 1),
        Err(SessionLineageError::UnsupportedSchemaVersion { seq: 1, .. })
    ));

    let mut run_mismatch = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];
    run_mismatch[1].run_id = crate::ids::RunId::from("run_other");
    assert!(matches!(
        validate_stable_prefix(&run_mismatch, 2),
        Err(SessionLineageError::RunIdMismatch { seq: 2, .. })
    ));
}

#[test]
fn session_lineage_rejects_unstable_prefixes() {
    // arrange
    // act
    // assert
    let active = vec![envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".into(),
            workspace_root: "/workspace".to_string(),
        }),
    )];
    assert!(matches!(
        validate_stable_prefix(&active, 1),
        Err(SessionLineageError::UnstablePrefix { reason, .. })
            if reason.contains("run is still active")
    ));

    let pending_permission = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "bash".to_string(),
                tool_call_id: None,
                summary: "run command".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 1000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];
    assert!(matches!(
        validate_stable_prefix(&pending_permission, 3),
        Err(SessionLineageError::UnstablePrefix { reason, .. })
            if reason.contains("pending permissions") && reason.contains("perm_000001")
    ));
}

#[test]
fn session_lineage_fork_rejects_running_source_without_stable_prefix() {
    // arrange — a source session with no terminal run lifecycle
    let events = vec![envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".into(),
            workspace_root: "/workspace".to_string(),
        }),
    )];

    // act
    let err = validate_fork_stable_prefix(&events, 1).expect_err("running source");

    // assert — fork fails closed on active sources, same as clone
    assert!(matches!(
        err,
        SessionLineageError::UnstablePrefix { cutoff_seq: 1, reason }
            if reason.contains("run is still active")
    ));
}

#[test]
fn session_lineage_clone_rejects_running_source_without_stable_prefix() {
    // arrange
    // act
    // assert
    let events = vec![envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".into(),
            workspace_root: "/workspace".to_string(),
        }),
    )];

    assert!(matches!(
        latest_clone_stable_prefix(&events),
        Err(SessionLineageError::UnstablePrefix { cutoff_seq: 1, reason })
            if reason.contains("no stable completed prefix")
    ));
}

#[test]
fn session_lineage_handles_first_last_and_out_of_range_cutoffs() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "bash".to_string(),
                tool_call_id: None,
                summary: "run command".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 1000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            3,
            EventV1::PermissionResolved(PermissionResolvedEvent {
                permission_id: "perm_000001".to_string(),
                decision: PermissionDecision::Allow,
                reason: Some("approved".to_string()),
            }),
        ),
        envelope(
            4,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    assert!(matches!(
        validate_stable_prefix(&events, 1),
        Err(SessionLineageError::UnstablePrefix { .. })
    ));
    assert_eq!(
        validate_stable_prefix(&events, 4)
            .unwrap_or_abort()
            .cutoff_seq,
        4
    );
    assert!(matches!(
        validate_stable_prefix(&events, 5),
        Err(SessionLineageError::CutoffOutOfRange {
            cutoff_seq: 5,
            max_seq: 4
        })
    ));
}

#[test]
fn session_lineage_treats_legacy_entries_without_parent_metadata_as_roots() {
    // arrange
    // act
    // assert
    let tree = project_lineage_tree(vec![
        entry("legacy-b", None, "2026-05-03T00:02:00Z"),
        entry("legacy-a", None, "2026-05-03T00:01:00Z"),
        entry("orphan", Some("missing-parent"), "2026-05-03T00:03:00Z"),
    ]);

    let flattened = tree
        .flatten()
        .into_iter()
        .map(|row| (row.depth, row.entry.run_id.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(
        flattened,
        vec![(0, "orphan"), (0, "legacy-b"), (0, "legacy-a")]
    );
}

#[test]
fn session_lineage_tracks_tool_call_in_flight_cutoffs() {
    // arrange
    // act
    // assert
    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_000001".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-tool".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished before tool result".to_string(),
            }),
        ),
        envelope(
            4,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000001".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("ok".to_string()),
                output_digest: Some("digest-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
    ];

    assert!(matches!(
        validate_stable_prefix(&events, 3),
        Err(SessionLineageError::UnstablePrefix { reason, .. })
            if reason.contains("tool calls are still in flight")
    ));
    assert_eq!(
        validate_stable_prefix(&events, 4)
            .unwrap_or_abort()
            .cutoff_seq,
        4
    );
}

#[test]
fn session_lineage_rejects_source_event_log_changed_while_materializing() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_session_lineage");
    fs::create_dir_all(&source_run_dir).unwrap_or_abort();
    let events = finished_events();
    write_events_jsonl(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).unwrap_or_abort();

    let mut changed_events = events.clone();
    changed_events.push(envelope(
        3,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "changed".into(),
            workspace_root: "/workspace".to_string(),
        }),
    ));

    let err = materialize_child_session_inner(
        ChildSessionMaterializationRequest {
            source_run_dir: &source_run_dir,
            events: &events,
            stable_prefix: &prefix,
            source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
        &SystemChildRunIdSource,
        None,
        || write_events_jsonl(&source_run_dir, &changed_events),
        |from, to| fs::rename(from, to),
    )
    .expect_err("changed source event log must reject before publish");

    assert!(matches!(
        err,
        ChildSessionMaterializationError::SourceEventLogChanged { .. }
    ));
    assert_eq!(
        session_dir_entries(temp_dir.path()),
        vec!["run_session_lineage"]
    );
    assert_no_unpublished_temp_dirs(temp_dir.path());
}

#[test]
fn session_lineage_destination_collision_cleans_temp_without_overwriting_existing_run() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_session_lineage");
    fs::create_dir_all(&source_run_dir).unwrap_or_abort();
    let events = finished_events();
    write_events_jsonl(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).unwrap_or_abort();
    let (child_run_id, child_run_dir, temp_run_dir) = planned_child_paths(temp_dir.path());
    fs::create_dir_all(&child_run_dir).unwrap_or_abort();
    fs::write(child_run_dir.join("existing.txt"), "existing child").unwrap_or_abort();

    let err = materialize_child_session_inner(
        ChildSessionMaterializationRequest {
            source_run_dir: &source_run_dir,
            events: &events,
            stable_prefix: &prefix,
            source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
        &SystemChildRunIdSource,
        Some((
            child_run_id.clone(),
            child_run_dir.clone(),
            temp_run_dir.clone(),
        )),
        || {},
        |from, to| fs::rename(from, to),
    )
    .expect_err("colliding child directory must reject publish");

    assert!(matches!(
        err,
        ChildSessionMaterializationError::PublishRunDirectory { .. }
    ));
    assert!(child_run_dir.join("existing.txt").exists());
    assert!(!child_run_dir.join("events.jsonl").exists());
    assert!(!temp_run_dir.exists());
    assert_no_unpublished_temp_dirs(temp_dir.path());
}

#[test]
fn session_lineage_cross_device_publish_error_cleans_temp_without_fallback() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_session_lineage");
    fs::create_dir_all(&source_run_dir).unwrap_or_abort();
    let events = finished_events();
    write_events_jsonl(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).unwrap_or_abort();
    let (child_run_id, child_run_dir, temp_run_dir) = planned_child_paths(temp_dir.path());

    let err = materialize_child_session_inner(
        ChildSessionMaterializationRequest {
            source_run_dir: &source_run_dir,
            events: &events,
            stable_prefix: &prefix,
            source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
        &SystemChildRunIdSource,
        Some((
            child_run_id.clone(),
            child_run_dir.clone(),
            temp_run_dir.clone(),
        )),
        || {},
        |_, _| Err(std::io::Error::from_raw_os_error(18)),
    )
    .expect_err("cross-device rename error must not fall back to a non-atomic copy");

    assert!(matches!(
        err,
        ChildSessionMaterializationError::PublishRunDirectory { .. }
    ));
    assert!(!child_run_dir.exists());
    assert!(!temp_run_dir.exists());
    assert_no_unpublished_temp_dirs(temp_dir.path());
}

#[test]
fn session_lineage_materialization_uses_injected_child_run_id_source() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let source_run_dir = temp_dir.path().join("run_session_lineage");
    fs::create_dir_all(&source_run_dir).unwrap_or_abort();
    let events = finished_events();
    write_events_jsonl(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).unwrap_or_abort();

    let result = materialize_child_session_inner(
        ChildSessionMaterializationRequest {
            source_run_dir: &source_run_dir,
            events: &events,
            stable_prefix: &prefix,
            source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
        },
        &StaticChildRunIdSource("run_harness_child_seeded"),
        None,
        || {},
        |from, to| fs::rename(from, to),
    )
    .unwrap_or_abort();

    assert_eq!(result.child_run_id, "run_harness_child_seeded");
    assert!(result.child_run_dir.ends_with("run_harness_child_seeded"));
    let child_events =
        fs::read_to_string(result.child_run_dir.join(EVENTS_FILE_NAME)).unwrap_or_abort();
    assert!(child_events.contains("run_harness_child_seeded"));
    assert!(child_events.contains("evt-run_harness_child_seeded-00000000000000000001"));
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_session_lineage".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_session_lineage".to_string()),
        payload,
    }
}

fn finished_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ]
}

fn write_events_jsonl(run_dir: &Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap_or_abort())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).unwrap_or_abort();
}

fn planned_child_paths(session_dir: &Path) -> (String, PathBuf, PathBuf) {
    let child_run_id = "run_harness_child_planned".to_string();
    (
        child_run_id.clone(),
        session_dir.join(&child_run_id),
        session_dir.join(format!(".{child_run_id}.tmp-planned")),
    )
}

fn session_dir_entries(session_dir: &Path) -> Vec<String> {
    let mut entries = fs::read_dir(session_dir)
        .unwrap_or_abort()
        .map(|entry| {
            entry
                .unwrap_or_abort()
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn assert_no_unpublished_temp_dirs(session_dir: &Path) {
    for entry in fs::read_dir(session_dir).unwrap_or_abort() {
        let entry = entry.unwrap_or_abort();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        assert!(
            !(name.starts_with(".run_harness_child") && name.contains(".tmp-")),
            "unpublished temp dir remained: {name}"
        );
    }
}

fn entry(
    run_id: &str,
    parent_session_id: Option<&str>,
    last_updated_at: &str,
) -> SessionCatalogEntry {
    SessionCatalogEntry {
        run_id: run_id.to_string(),
        run_name: Some(run_id.to_string()),
        status: Some(RunStatus::Finished),
        last_updated_at: Some(last_updated_at.to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: Some("default".to_string()),
        provider_model: Some("default/gpt-5".to_string()),
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: parent_session_id.map(str::to_string),
    }
}
