use std::collections::BTreeMap;
use std::sync::Arc;

use harness_core::agent::AgentProfile;
use harness_core::clock::FakeClock;
use harness_core::config::{ShellAllowlist, ToolFailureMode};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::EventV1;
use harness_core::proj::{TeamMemberStatus, TeamRunStatus};
use harness_core::redact::DefaultRedactor;
use harness_tools::coordinator_registry;
use serde_json::json;

mod common;

use common::{anonymous_supervisor_actor, read_events};

#[tokio::test]
async fn native_team_tools_append_events_and_return_projected_state() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.run_id_override = Some("run_tool_team".to_string());
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([("alpha".to_string(), profile("alpha"))]);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("tool-team", tempdir.path())
        .await
        .expect("start run");
    let actor = anonymous_supervisor_actor();

    let created = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_create",
            json!({
                "teamRunId": "team_tool",
                "name": "tool-team",
                "members": [{
                    "name": "alpha",
                    "kind": "subagent_type",
                    "subagent_type": "alpha"
                }]
            }),
        )
        .await
        .expect("team_create tool");
    assert!(created.display_text.contains("team created"));

    let team_list = handle
        .execute_agent_tool_call(actor.clone(), None, "team_list", json!({}))
        .await
        .expect("team_list tool");
    assert!(team_list.display_text.contains("1 active team(s)"));
    let team_list_json = team_list
        .structured_json
        .expect("team_list structured json");
    assert_eq!(team_list_json["teams"][0]["team_run_id"], "team_tool");
    assert_eq!(team_list_json["source"], "event_replay");

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_send_message",
            json!({
                "teamRunId": "team_tool",
                "from": "lead",
                "to": "alpha",
                "body": "hello"
            }),
        )
        .await
        .expect("team_send_message tool");

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_create",
            json!({
                "teamRunId": "team_tool",
                "taskId": "task_a",
                "subject": "A",
                "description": "Do A"
            }),
        )
        .await
        .expect("team_task_create tool");
    let updated = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_update",
            json!({
                "teamRunId": "team_tool",
                "taskId": "task_a",
                "status": "claimed",
                "owner": "alpha"
            }),
        )
        .await
        .expect("team_task_update tool");
    assert!(updated.display_text.contains("team task updated"));

    let listed = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_list",
            json!({
                "teamRunId": "team_tool",
                "status": "claimed",
                "owner": "alpha"
            }),
        )
        .await
        .expect("team_task_list tool");
    assert!(listed.display_text.contains("1 task(s)"));

    let fetched = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_get",
            json!({
                "teamRunId": "team_tool",
                "taskId": "task_a"
            }),
        )
        .await
        .expect("team_task_get tool");
    assert!(fetched.display_text.contains("team task: task_a"));

    let premature_delete = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_delete",
            json!({ "teamRunId": "team_tool" }),
        )
        .await
        .expect_err("team_delete requires shutdown approval");
    assert!(premature_delete
        .to_string()
        .contains("before shutdown approval"));

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_request",
            json!({
                "teamRunId": "team_tool",
                "memberName": "alpha",
                "requester": "lead"
            }),
        )
        .await
        .expect("team_shutdown_request tool");

    let approved = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_approve",
            json!({
                "teamRunId": "team_tool",
                "memberName": "alpha",
                "actorName": "alpha"
            }),
        )
        .await
        .expect("team_shutdown_approve tool");
    assert!(approved.display_text.contains("team shutdown approved"));

    let deleted = handle
        .execute_agent_tool_call(
            actor,
            None,
            "team_delete",
            json!({ "teamRunId": "team_tool" }),
        )
        .await
        .expect("team_delete tool");
    assert!(deleted.display_text.contains("team deleted"));

    let events = read_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamMessageSent(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamTaskUpdated(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamShutdownApproved(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::TeamDeleted(_))));
}

#[tokio::test]
async fn native_persistent_task_tools_append_events_and_project_dependencies() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.run_id_override = Some("run_tool_persistent_tasks".to_string());
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("tool-persistent-tasks", tempdir.path())
        .await
        .expect("start run");
    let actor = anonymous_supervisor_actor();

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "task_create",
            json!({
                "task_id": "task_a",
                "subject": "A",
                "description": "Do A"
            }),
        )
        .await
        .expect("task_create task_a");
    let created_b = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "task_create",
            json!({
                "task_id": "task_b",
                "subject": "B",
                "description": "Do B",
                "blocked_by": ["task_a"]
            }),
        )
        .await
        .expect("task_create task_b");
    assert!(created_b.display_text.contains("persistent task created"));

    let blocked = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "task_update",
            json!({
                "task_id": "task_b",
                "status": "in_progress"
            }),
        )
        .await
        .expect_err("dependency blocks task_b");
    assert!(blocked.to_string().contains("blocked by incomplete task"));

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "task_update",
            json!({
                "task_id": "task_a",
                "status": "completed"
            }),
        )
        .await
        .expect("complete task_a");
    let updated_b = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "task_update",
            json!({
                "task_id": "task_b",
                "status": "in_progress",
                "owner": "build",
                "active_form": "Implement B"
            }),
        )
        .await
        .expect("update task_b");
    assert_eq!(
        updated_b.structured_json.as_ref().unwrap()["task"]["owner"],
        "build"
    );

    let list = handle
        .execute_agent_tool_call(actor.clone(), None, "task_list", json!({}))
        .await
        .expect("task_list");
    let list_json = list.structured_json.expect("task_list json");
    assert_eq!(list_json["tasks"].as_array().expect("tasks").len(), 2);
    assert_eq!(list_json["tasks"][0]["blocks"], json!(["task_b"]));
    assert_eq!(list_json["ready_task_ids"], json!([]));

    let got = handle
        .execute_agent_tool_call(actor, None, "task_get", json!({ "task_id": "task_b" }))
        .await
        .expect("task_get");
    assert_eq!(
        got.structured_json.unwrap()["task"]["active_form"],
        "Implement B"
    );

    let events = read_events(&run.events_path);
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::PersistentTaskCreated(_))));
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::PersistentTaskUpdated(_))));
}

#[tokio::test]
async fn team_list_reports_declared_team_specs_with_validation() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let teams_dir = tempdir.path().join(".agent-harness/teams");
    std::fs::create_dir_all(&teams_dir).expect("teams dir");
    std::fs::write(
        teams_dir.join("triage.json"),
        r#"
        {
          "version": 1,
          "name": "triage",
          "description": "Triage team",
          "lead": { "kind": "subagent_type", "subagent_type": "alpha" },
          "members": [
            {
              "name": "research",
              "role": "research",
              "selector": { "kind": "subagent_type", "subagent_type": "oracle" }
            },
            {
              "name": "builder",
              "selector": { "kind": "subagent_type", "subagent_type": "alpha" }
            }
          ],
          "bounds": {
            "max_members": 4,
            "max_parallel_members": 2,
            "max_messages_per_run": 20,
            "max_wall_clock_minutes": 30,
            "max_member_turns": 10
          }
        }
        "#,
    )
    .expect("team spec");
    std::fs::write(
        teams_dir.join("bad.json"),
        r#"
        {
          "version": 1,
          "name": "bad",
          "lead": { "kind": "subagent_type", "subagent_type": "oracle" },
          "members": []
        }
        "#,
    )
    .expect("invalid team spec");

    let mut config = CoordinatorConfig::new(tempdir.path().join("sessions"));
    config.run_id_override = Some("run_declared_team_list".to_string());
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([("alpha".to_string(), profile("alpha"))]);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle
        .start_run("declared-team-list", tempdir.path())
        .await
        .expect("start run");

    let listed = handle
        .execute_agent_tool_call(anonymous_supervisor_actor(), None, "team_list", json!({}))
        .await
        .expect("team_list tool");
    let output = listed.structured_json.expect("team_list json");
    assert_eq!(
        output["declared_teams"].as_array().expect("declared").len(),
        2
    );
    let triage = output["declared_teams"]
        .as_array()
        .expect("declared")
        .iter()
        .find(|team| team["name"] == "triage")
        .expect("triage");
    assert_eq!(triage["status"], "valid");
    let bad = output["declared_teams"]
        .as_array()
        .expect("declared")
        .iter()
        .find(|team| team["name"] == "bad")
        .expect("bad");
    assert_eq!(bad["status"], "invalid");
    assert_eq!(output["declared_team_validation"]["invalid"], 1);
}

#[tokio::test]
async fn native_team_tools_enforce_task_and_shutdown_ordering() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.run_id_override = Some("run_tool_team_ordering".to_string());
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([
        ("alpha".to_string(), profile("alpha")),
        ("beta".to_string(), profile("beta")),
    ]);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle
        .start_run("tool-team-ordering", tempdir.path())
        .await
        .expect("start run");
    let actor = anonymous_supervisor_actor();

    let created = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_create",
            json!({
                "teamRunId": "team_tool_ordering",
                "name": "tool-team-ordering",
                "members": [
                    {
                        "name": "alpha",
                        "kind": "subagent_type",
                        "subagent_type": "alpha"
                    },
                    {
                        "name": "beta",
                        "kind": "subagent_type",
                        "subagent_type": "beta"
                    }
                ]
            }),
        )
        .await
        .expect("team_create tool");
    let created_json = created.structured_json.expect("created structured json");
    assert_eq!(
        created_json["members"]["alpha"]["status"],
        json!(TeamMemberStatus::Running)
    );
    assert!(created_json["members"]["alpha"]["agent_id"].is_string());
    assert_eq!(
        created_json["members"]["beta"]["status"],
        json!(TeamMemberStatus::Running)
    );

    let missing_approval = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_approve",
            json!({
                "teamRunId": "team_tool_ordering",
                "memberName": "alpha",
                "actorName": "alpha"
            }),
        )
        .await
        .expect_err("approval requires pending shutdown request");
    assert!(missing_approval
        .to_string()
        .contains("no pending shutdown request"));

    let missing_rejection = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_reject",
            json!({
                "teamRunId": "team_tool_ordering",
                "memberName": "alpha",
                "actorName": "alpha",
                "reason": "still running"
            }),
        )
        .await
        .expect_err("rejection requires pending shutdown request");
    assert!(missing_rejection
        .to_string()
        .contains("no pending shutdown request"));

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_send_message",
            json!({
                "teamRunId": "team_tool_ordering",
                "from": "lead",
                "to": "alpha",
                "body": "take task one before task two"
            }),
        )
        .await
        .expect("team_send_message tool");
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_create",
            json!({
                "teamRunId": "team_tool_ordering",
                "taskId": "task_one",
                "subject": "one",
                "description": "first task"
            }),
        )
        .await
        .expect("team_task_create task_one tool");
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_create",
            json!({
                "teamRunId": "team_tool_ordering",
                "taskId": "task_two",
                "subject": "two",
                "description": "second task",
                "blockedBy": ["task_one"]
            }),
        )
        .await
        .expect("team_task_create task_two tool");

    let blocked_claim = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_update",
            json!({
                "teamRunId": "team_tool_ordering",
                "taskId": "task_two",
                "status": "claimed",
                "owner": "alpha"
            }),
        )
        .await
        .expect_err("blocked task cannot be claimed");
    assert!(blocked_claim
        .to_string()
        .contains("blocked by incomplete tasks"));

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_update",
            json!({
                "teamRunId": "team_tool_ordering",
                "taskId": "task_one",
                "status": "completed",
                "owner": "alpha"
            }),
        )
        .await
        .expect("complete blocker");
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_update",
            json!({
                "teamRunId": "team_tool_ordering",
                "taskId": "task_two",
                "status": "claimed",
                "owner": "alpha"
            }),
        )
        .await
        .expect("claim unblocked task");

    let listed = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_list",
            json!({
                "teamRunId": "team_tool_ordering",
                "status": "claimed",
                "owner": "alpha"
            }),
        )
        .await
        .expect("team_task_list claimed tool");
    assert!(listed.display_text.contains("1 task(s)"));
    let listed_json = listed.structured_json.expect("listed structured json");
    assert_eq!(listed_json["tasks"][0]["task_id"], "task_two");

    let fetched = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_task_get",
            json!({
                "teamRunId": "team_tool_ordering",
                "taskId": "task_two"
            }),
        )
        .await
        .expect("team_task_get task_two tool");
    let fetched_json = fetched.structured_json.expect("fetched structured json");
    assert_eq!(fetched_json["status"], "claimed");
    assert_eq!(fetched_json["owner"], "alpha");

    let premature_delete = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_delete",
            json!({ "teamRunId": "team_tool_ordering" }),
        )
        .await
        .expect_err("team_delete requires shutdown approval");
    assert!(premature_delete
        .to_string()
        .contains("before shutdown approval"));

    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_request",
            json!({
                "teamRunId": "team_tool_ordering",
                "memberName": "alpha",
                "requester": "lead"
            }),
        )
        .await
        .expect("request alpha shutdown");
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_reject",
            json!({
                "teamRunId": "team_tool_ordering",
                "memberName": "alpha",
                "actorName": "alpha",
                "reason": "more work"
            }),
        )
        .await
        .expect("reject alpha shutdown");
    let stale_rejection = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_reject",
            json!({
                "teamRunId": "team_tool_ordering",
                "memberName": "alpha",
                "actorName": "alpha",
                "reason": "again"
            }),
        )
        .await
        .expect_err("stale rejection requires fresh request");
    assert!(stale_rejection
        .to_string()
        .contains("no pending shutdown request"));

    for member in ["alpha", "beta"] {
        handle
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "team_shutdown_request",
                json!({
                    "teamRunId": "team_tool_ordering",
                    "memberName": member,
                    "requester": "lead"
                }),
            )
            .await
            .expect("request member shutdown");
        handle
            .execute_agent_tool_call(
                actor.clone(),
                None,
                "team_shutdown_approve",
                json!({
                    "teamRunId": "team_tool_ordering",
                    "memberName": member,
                    "actorName": member
                }),
            )
            .await
            .expect("approve member shutdown");
    }

    let deleted = handle
        .execute_agent_tool_call(
            actor,
            None,
            "team_delete",
            json!({ "teamRunId": "team_tool_ordering" }),
        )
        .await
        .expect("team_delete tool");
    let deleted_json = deleted.structured_json.expect("deleted structured json");
    assert_eq!(deleted_json["status"], json!(TeamRunStatus::Deleted));
}

#[tokio::test]
async fn native_team_create_accepts_research_role_but_mutations_stay_coordinator_gated() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let mut config = CoordinatorConfig::new(tempdir.path());
    config.run_id_override = Some("run_tool_team_research".to_string());
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = BTreeMap::from([("explore".to_string(), profile("explore"))]);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle
        .start_run("tool-team-research", tempdir.path())
        .await
        .expect("start run");
    let actor = anonymous_supervisor_actor();

    let created = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_create",
            json!({
                "teamRunId": "team_tool_research",
                "name": "tool-team-research",
                "members": [{
                    "name": "research",
                    "role": "research",
                    "kind": "subagent_type",
                    "subagent_type": "explore"
                }]
            }),
        )
        .await
        .expect("team_create research tool");
    assert_eq!(
        created.structured_json.as_ref().unwrap()["members"]["research"]["role"],
        "research"
    );

    let denied = handle
        .execute_agent_tool_call(
            actor,
            None,
            "team_send_message",
            json!({
                "teamRunId": "team_tool_research",
                "from": "research",
                "to": "lead",
                "body": "research report"
            }),
        )
        .await
        .expect_err("research member cannot mutate team mailbox");
    assert!(denied.to_string().contains("research team member"));
}

fn profile(name: &str) -> AgentProfile {
    AgentProfile {
        name: name.to_string(),
        category: "deep".to_string(),
        model_ref: "mock:model-1".to_string(),
        model_ref_explicit: true,
        fallback_model_refs: Vec::new(),
        fallback_model_settings: Vec::new(),
        system_prompt: format!("{name}-prompt"),
        max_iters: Some(1),
        temperature: Some(0.0),
        tool_failure_mode: ToolFailureMode::FailTurn,
        toolset: vec!["team_status".to_string()],
    }
}
