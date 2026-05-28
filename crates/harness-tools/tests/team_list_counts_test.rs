use serde_json::{json, Value};

mod common;

use common::{anonymous_supervisor_actor, team_coordinator};

async fn create_team(
    handle: &harness_core::coord::CoordinatorHandle,
    actor: harness_core::event::EventActor,
    team_run_id: &str,
    name: &str,
) {
    handle
        .execute_agent_tool_call(
            actor,
            None,
            "team_create",
            json!({
                "teamRunId": team_run_id,
                "name": name,
                "members": [{
                    "name": "alpha",
                    "kind": "subagent_type",
                    "subagent_type": "alpha"
                }]
            }),
        )
        .await
        .expect("team_create tool");
}

async fn list_teams(
    handle: &harness_core::coord::CoordinatorHandle,
    actor: harness_core::event::EventActor,
    args: Value,
) -> Value {
    handle
        .execute_agent_tool_call(actor, None, "team_list", args)
        .await
        .expect("team_list tool")
        .structured_json
        .expect("team_list json")
}

#[tokio::test]
async fn team_list_counts_total_filtered_returned_and_truncated_runs() {
    // arrange
    let tempdir = tempfile::tempdir().expect("tempdir");
    let handle = team_coordinator(tempdir.path(), "run_tool_team_counts", &["alpha"]);
    handle
        .start_run("tool-team-counts", tempdir.path())
        .await
        .expect("start run");
    let actor = anonymous_supervisor_actor();
    create_team(&handle, actor.clone(), "team_active", "active-team").await;
    create_team(&handle, actor.clone(), "team_shutdown", "shutdown-team").await;
    handle
        .execute_agent_tool_call(
            actor.clone(),
            None,
            "team_shutdown_request",
            json!({
                "teamRunId": "team_shutdown",
                "memberName": "alpha",
                "requester": "lead"
            }),
        )
        .await
        .expect("team_shutdown_request tool");

    // act
    let unfiltered = list_teams(&handle, actor.clone(), json!({"limit": 10})).await;
    let shutdown_requested = list_teams(
        &handle,
        actor.clone(),
        json!({"status": "shutdown_requested", "limit": 10}),
    )
    .await;
    let limited = list_teams(&handle, actor, json!({"limit": 1})).await;

    // assert
    assert_eq!(unfiltered["total_count"], json!(2));
    assert_eq!(unfiltered["filtered_count"], json!(2));
    assert_eq!(unfiltered["returned_count"], json!(2));
    assert_eq!(unfiltered["truncated_count"], json!(0));
    assert_eq!(unfiltered["truncated"], json!(false));
    assert_eq!(shutdown_requested["total_count"], json!(2));
    assert_eq!(shutdown_requested["filtered_count"], json!(1));
    assert_eq!(shutdown_requested["returned_count"], json!(1));
    assert_eq!(
        shutdown_requested["teams"][0]["team_run_id"],
        json!("team_shutdown")
    );
    assert_eq!(limited["total_count"], json!(2));
    assert_eq!(limited["filtered_count"], json!(2));
    assert_eq!(limited["returned_count"], json!(1));
    assert_eq!(limited["truncated_count"], json!(1));
    assert_eq!(limited["truncated"], json!(true));
}
