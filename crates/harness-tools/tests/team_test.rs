use harness_core::event::EventV1;
use harness_core::proj::{TeamMemberStatus, TeamRunStatus};
use serde_json::json;

mod common;

use common::{anonymous_supervisor_actor, read_events, team_coordinator};

#[tokio::test]
async fn native_team_tools_append_events_and_return_projected_state() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let handle = team_coordinator(tempdir.path(), "run_tool_team", &["alpha"]);
    let run = handle
        .start_run("tool-team", tempdir.path())
        .await
        .expect("start run");
    let actor = anonymous_supervisor_actor();

    let empty_list = handle
        .execute_agent_tool_call(actor.clone(), None, "team_list", json!({}))
        .await
        .expect("empty team_list tool");
    let empty_list_json = empty_list.structured_json.expect("empty team_list json");
    assert_eq!(empty_list_json["returned_count"], json!(0));
    assert_eq!(empty_list_json["mutates"], json!(false));

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

    let team_list = handle
        .execute_agent_tool_call(actor.clone(), None, "team_list", json!({"limit": 10}))
        .await
        .expect("team_list tool");
    let team_list_json = team_list.structured_json.expect("team_list json");
    assert_eq!(
        team_list_json["scope"],
        json!("primitive_projection_reader")
    );
    assert_eq!(
        team_list_json
            .get("teams")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(team_list_json["mutates"], json!(false));
    let capped_team_list = handle
        .execute_agent_tool_call(actor.clone(), None, "team_list", json!({"limit": 999}))
        .await
        .expect("team_list capped");
    let capped_team_list_json = capped_team_list
        .structured_json
        .expect("team_list capped json");
    assert_eq!(capped_team_list_json["requested_limit"], json!(999));
    assert_eq!(capped_team_list_json["effective_limit"], json!(200));
    assert_eq!(capped_team_list_json["max_limit"], json!(200));
    assert_eq!(capped_team_list_json["limit_clamped"], json!(true));

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

    let shutdown_requested_list = handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_list",
            json!({"status": "shutdown_requested"}),
        )
        .await
        .expect("team_list shutdown requested");
    let shutdown_requested_json = shutdown_requested_list
        .structured_json
        .expect("team_list shutdown requested json");
    assert_eq!(shutdown_requested_json["returned_count"], json!(1));
    assert_eq!(
        shutdown_requested_json["teams"][0]["shutdown_state"],
        json!("shutdown_requested")
    );

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
            actor.clone(),
            None,
            "team_delete",
            json!({ "teamRunId": "team_tool" }),
        )
        .await
        .expect("team_delete tool");
    assert!(deleted.display_text.contains("team deleted"));

    let deleted_list = handle
        .execute_agent_tool_call(actor, None, "team_list", json!({"status": "deleted"}))
        .await
        .expect("team_list deleted");
    let deleted_list_json = deleted_list
        .structured_json
        .expect("team_list deleted json");
    assert_eq!(deleted_list_json["returned_count"], json!(1));
    assert_eq!(deleted_list_json["teams"][0]["deleted"], json!(true));

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
async fn native_team_tools_enforce_task_and_shutdown_ordering() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let handle = team_coordinator(tempdir.path(), "run_tool_team_ordering", &["alpha", "beta"]);
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
    let handle = team_coordinator(tempdir.path(), "run_tool_team_research", &["explore"]);
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
