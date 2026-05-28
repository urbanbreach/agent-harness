use std::fs;
use std::path::Path;

use harness_core::config::ShellAllowlist;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFailedEvent, RunStartedEvent,
    TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata, TeamBounds, TeamCreatedEvent,
    TeamMemberRole, TeamMemberSelector, TeamMemberSpec, TeamSpec, SCHEMA_VERSION,
};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};

mod common;

use common::{setup_workspace_fixture, test_context};

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, None),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_session_events(workspace: &Path, run_id: &str, events: &[EventEnvelopeV1]) {
    let run_dir = workspace.join(".agent-harness/sessions").join(run_id);
    fs::create_dir_all(&run_dir).expect("session run dir");
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("events jsonl");
}

fn write_session_metadata(workspace: &Path, run_id: &str, metadata: Value) {
    let run_dir = workspace.join(".agent-harness/sessions").join(run_id);
    fs::create_dir_all(&run_dir).expect("session run dir");
    fs::write(
        run_dir.join("meta.json"),
        serde_json::to_string_pretty(&metadata).expect("metadata json"),
    )
    .expect("session metadata");
}

async fn session_info(workspace: &Path, run_id: &str, selector: &str) -> Value {
    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(workspace, run_id, "session-info-test");
    registry
        .get("session_info")
        .expect("session_info")
        .call(ctx, json!({"session": selector}))
        .await
        .expect("session_info call")
        .structured_json
        .expect("session_info json")
}

fn run_started(run_id: &str, seq: u64, run_name: &str, workspace: &Path) -> EventEnvelopeV1 {
    envelope(
        run_id,
        seq,
        EventV1::RunStarted(RunStartedEvent {
            run_name: run_name.to_string(),
            workspace_root: workspace.display().to_string(),
        }),
    )
}

fn team_created(run_id: &str, seq: u64, team_run_id: &str) -> EventEnvelopeV1 {
    envelope(
        run_id,
        seq,
        EventV1::TeamCreated(TeamCreatedEvent {
            team_run_id: team_run_id.to_string(),
            spec: TeamSpec {
                version: 1,
                name: team_run_id.to_string(),
                description: None,
                lead: None,
                members: vec![TeamMemberSpec {
                    name: "alpha".to_string(),
                    role: TeamMemberRole::Member,
                    selector: TeamMemberSelector::SubagentType {
                        subagent_type: "general".to_string(),
                    },
                    prompt: None,
                }],
                bounds: TeamBounds::default(),
            },
        }),
    )
}

#[tokio::test]
async fn session_info_reports_failed_replay_only_child_lineage_and_missing_sessions() {
    // arrange
    let workspace = setup_workspace_fixture();
    write_session_events(
        workspace.workspace(),
        "run_failed_info",
        &[
            run_started("run_failed_info", 1, "failed info", workspace.workspace()),
            envelope(
                "run_failed_info",
                2,
                EventV1::RunFailed(RunFailedEvent {
                    error: "provider failed closed".to_string(),
                }),
            ),
        ],
    );
    write_session_events(
        workspace.workspace(),
        "run_replay_only",
        &[run_started(
            "run_replay_only",
            1,
            "replay only",
            workspace.workspace(),
        )],
    );
    write_session_events(
        workspace.workspace(),
        "run_child_lineage",
        &[
            run_started(
                "run_child_lineage",
                1,
                "child lineage",
                workspace.workspace(),
            ),
            envelope(
                "run_child_lineage",
                2,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_1".to_string(),
                    result_summary: "child done".to_string(),
                    result_digest: "digest-child".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_session_id: Some("run_parent".to_string()),
                            child_session_id: Some("run_child_lineage".to_string()),
                            ..TaskLineageMetadata::default()
                        }),
                        ..TaskCompletionMetadata::default()
                    }),
                }),
            ),
        ],
    );
    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-info-missing",
        "session-info-test",
    );

    // act
    let failed = session_info(
        workspace.workspace(),
        "run-session-info-failed",
        "run_failed_info",
    )
    .await;
    let replay_only = session_info(
        workspace.workspace(),
        "run-session-info-replay",
        "run_replay_only",
    )
    .await;
    let child = session_info(
        workspace.workspace(),
        "run-session-info-child",
        "run_child_lineage",
    )
    .await;
    let missing = registry
        .get("session_info")
        .expect("session_info")
        .call(ctx, json!({"session": "missing_session"}))
        .await
        .expect_err("missing session should fail");

    // assert
    assert_eq!(failed.pointer("/catalog/status"), Some(&json!("failed")));
    assert_eq!(failed.pointer("/event_counts/run_failed"), Some(&json!(1)));
    assert_eq!(replay_only.pointer("/source"), Some(&json!("event_replay")));
    assert!(replay_only.get("metadata").is_some_and(Value::is_null));
    assert_eq!(
        child.pointer("/lineage/parent_session_id"),
        Some(&json!("run_parent"))
    );
    assert_eq!(
        child.pointer("/lineage/child_session_count"),
        Some(&json!(1))
    );
    assert!(missing
        .to_string()
        .contains("unknown session `missing_session`"));
}

#[tokio::test]
async fn session_info_caps_team_projection_and_spills_large_payloads() {
    // arrange
    let workspace = setup_workspace_fixture();
    let mut team_events = vec![run_started(
        "run_many_teams",
        1,
        "many teams",
        workspace.workspace(),
    )];
    for index in 0..30 {
        team_events.push(team_created(
            "run_many_teams",
            index + 2,
            &format!("team_{index:02}"),
        ));
    }
    write_session_events(workspace.workspace(), "run_many_teams", &team_events);
    write_session_events(
        workspace.workspace(),
        "run_large_info",
        &[run_started(
            "run_large_info",
            1,
            "large info",
            workspace.workspace(),
        )],
    );
    write_session_metadata(
        workspace.workspace(),
        "run_large_info",
        json!({
            "run_id": "run_large_info",
            "run_name": "large info",
            "workspace_root": workspace.workspace(),
            "profile_preset": "build",
            "provider": "default",
            "model": format!("model-{}", "x".repeat(30_000)),
            "mode_source": "prompt"
        }),
    );
    let registry = coordinator_registry(ShellAllowlist::default());
    let ctx = test_context(
        workspace.workspace(),
        "run-session-info-spill",
        "session-info-spill",
    );

    // act
    let capped = session_info(
        workspace.workspace(),
        "run-session-info-cap",
        "run_many_teams",
    )
    .await;
    let spilled = registry
        .get("session_info")
        .expect("session_info")
        .call(ctx, json!({"session": "run_large_info"}))
        .await
        .expect("spilled session_info");

    // assert
    assert_eq!(capped.pointer("/team_count"), Some(&json!(30)));
    assert_eq!(capped.pointer("/team_limit"), Some(&json!(25)));
    assert_eq!(capped.pointer("/returned_team_count"), Some(&json!(25)));
    assert_eq!(capped.pointer("/team_truncated_count"), Some(&json!(5)));
    assert_eq!(capped.pointer("/teams_truncated"), Some(&json!(true)));
    assert_eq!(
        capped.get("teams").and_then(Value::as_array).map(Vec::len),
        Some(25)
    );
    assert_eq!(spilled.artifacts.len(), 1);
    let spill_json = spilled.structured_json.expect("spilled structured json");
    assert_eq!(spill_json.pointer("/spilled"), Some(&json!(true)));
    let artifact_path = spilled.artifacts[0].path.clone();
    let artifact_body = fs::read_to_string(workspace.workspace().join(&artifact_path))
        .expect("spilled artifact body");
    assert!(artifact_body.contains("run_large_info"));
    assert!(artifact_body.contains("model-"));
}
