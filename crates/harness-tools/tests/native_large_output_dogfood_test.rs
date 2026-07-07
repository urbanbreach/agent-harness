use harness_tools::UnwrapOrAbort;
use std::fs;

mod common;
#[path = "support/native_large_output_dogfood_support.rs"]
mod native_large_output_dogfood_support;

use common::{
    find_finished, read_events, setup_workspace_fixture, test_context, wait_for_tool_call_finish,
};
use harness_core::config::ShellAllowlist;
use harness_core::event::{
    EventV1, ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent,
};
use harness_tools::coordinator_registry;
use native_large_output_dogfood_support::{
    envelope, read_artifact, run_started, spawn_task_run, task_tool_actor, write_session_events,
};
use serde_json::{json, Value};

#[tokio::test]
async fn read_grep_and_bash_spill_large_outputs_with_model_guidance() {
    // arrange: large synthetic workspace outputs for the native coordinator registry.
    let workspace = setup_workspace_fixture();
    let large_read = (1..=2_050)
        .map(|line| format!("read-line-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        workspace.workspace().join("large-read.txt"),
        format!("{large_read}\n"),
    )
    .unwrap_or_abort();
    let large_grep = (1..=105)
        .map(|line| format!("MATCH grep-line-{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        workspace.workspace().join("large-grep.txt"),
        format!("{large_grep}\n"),
    )
    .unwrap_or_abort();
    let registry = coordinator_registry(ShellAllowlist::default());

    // act: read, grep, and bash each exceed their inline result budget.
    let read = registry
        .get("read")
        .unwrap_or_abort()
        .call(
            test_context(
                workspace.workspace(),
                "run-large-read",
                "toolcall-large-read",
            ),
            json!({"filePath": "large-read.txt"}),
        )
        .await
        .unwrap_or_abort();
    let grep = registry
        .get("grep")
        .unwrap_or_abort()
        .call(
            test_context(
                workspace.workspace(),
                "run-large-grep",
                "toolcall-large-grep",
            ),
            json!({"pattern": "MATCH", "path": "large-grep.txt"}),
        )
        .await
        .unwrap_or_abort();
    let bash = registry
        .get("bash")
        .unwrap_or_abort()
        .call(
            test_context(workspace.workspace(), "run-large-bash", "toolcall-large-bash"),
            json!({"command": "yes alpha | tr -d '\\n' | head -c 55000", "description": "large deterministic stdout"}),
        )
        .await
        .unwrap_or_abort();

    // assert: every surface keeps inline text bounded while pointing at full artifacts.
    assert_eq!(read.artifacts.len(), 1);
    assert!(read.display_text.contains("full output artifact:"));
    assert!(read.display_text.contains("Use offset="));
    let read_json = read.structured_json.unwrap_or_abort();
    assert_eq!(read_json["truncated"], json!(true));
    assert!(
        read_json["next_offset"].as_u64().unwrap_or_abort() > 1,
        "read should continue from a non-trivial next offset"
    );
    assert_eq!(
        read_json["output_artifact"]["path"],
        json!(read.artifacts[0].path)
    );

    assert_eq!(grep.artifacts.len(), 1);
    assert!(grep.display_text.contains("full output artifact:"));
    assert!(grep.display_text.contains("narrow by path/include/pattern"));
    assert!(!grep.display_text.contains("grep-line-105"));
    let grep_json = grep.structured_json.unwrap_or_abort();
    assert_eq!(grep_json["truncated"], json!(true));
    assert_eq!(
        grep_json["output_artifact"]["path"],
        json!(grep.artifacts[0].path)
    );
    assert!(
        read_artifact(workspace.workspace(), &grep.artifacts[0].path).contains("grep-line-105")
    );

    assert_eq!(bash.artifacts.len(), 1);
    assert!(bash.display_text.contains("[truncated:"));
    assert!(bash.display_text.contains("full output:"));
    assert!(bash.display_text.len() < 52_000);
    let bash_json = bash.structured_json.unwrap_or_abort();
    assert_eq!(bash_json["truncated"], json!(true));
    assert_eq!(
        bash_json["output_artifact"]["path"],
        json!(bash.artifacts[0].path)
    );
    assert_eq!(
        read_artifact(workspace.workspace(), &bash.artifacts[0].path).len(),
        55_000
    );
}

#[tokio::test]
async fn session_read_spills_redacted_replay_only_large_sessions() {
    // arrange: an event-log-like session with a command-looking event and secret-shaped text.
    let workspace = setup_workspace_fixture();
    let run_id = "run_large_session_read";
    let marker = workspace
        .workspace()
        .join("should-not-exist-from-session-read");
    let mut events = vec![run_started(run_id, workspace.workspace())];
    events.push(envelope(
        run_id,
        2,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-bash-replay-only".to_string(),
            tool_id: "bash".to_string(),
            args_summary: format!("touch {}", marker.display()),
            args_digest: "digest".to_string(),
            metadata: None,
        }),
    ));
    for seq in 3..=130 {
        events.push(envelope(
            run_id,
            seq,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: format!("req-{seq}"),
                text: format!("message-{seq} sk-test-{}", "a".repeat(80)),
            }),
        ));
    }
    write_session_events(workspace.workspace(), run_id, &events);
    let registry = coordinator_registry(ShellAllowlist::default());

    // act: session_read summarizes the session through replay-derived native tooling.
    let result = registry
        .get("session_read")
        .unwrap_or_abort()
        .call(
            test_context(
                workspace.workspace(),
                "run-session-read-dogfood",
                "toolcall-session-read",
            ),
            json!({"session": run_id, "eventLimit": 130, "messageLimit": 130}),
        )
        .await
        .unwrap_or_abort();

    // assert: the result spills redacted JSON and the command-looking event was not executed.
    assert_eq!(result.artifacts.len(), 1);
    assert!(result.display_text.contains("full output spilled to"));
    assert!(!marker.exists());
    let json = result.structured_json.unwrap_or_abort();
    assert_eq!(json["source"], json!("event_replay"));
    assert_eq!(json["spilled"], json!(true));
    assert_eq!(json["artifact"]["path"], json!(result.artifacts[0].path));
    let artifact = read_artifact(workspace.workspace(), &result.artifacts[0].path);
    assert!(artifact.contains(run_id));
    assert!(!artifact.contains("sk-test-"));
}

#[tokio::test]
async fn child_task_returns_capped_summary_and_session_next_actions() {
    // arrange: a foreground child task backed by a deterministic non-live provider.
    let workspace = setup_workspace_fixture();
    let (handle, run, worker_id) = spawn_task_run(workspace.workspace()).await;

    // act: the parent invokes the real task tool through the coordinator.
    let tool_call_id = handle
        .request_tool_call(
            task_tool_actor(&worker_id),
            Some("parent".to_string()),
            "task",
            json!({
                "subagent_type": "child",
                "description": "Large child summary",
                "prompt": "Return the deterministic large response.",
                "run_in_background": false,
                "load_skills": []
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;
    handle.stop_run().await.unwrap_or_abort();

    // assert: parent-visible task output is capped and points to session inspection tools.
    let finished = find_finished(&read_events(&run.events_path), &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["status"], json!("completed"));
    assert_capped_child_summary(&output);
    assert_next_action_tool(&output, "session_info");
    assert_next_action_tool(&output, "session_read");
}

fn assert_capped_child_summary(output: &Value) {
    let summary = output["result_summary"].as_str().unwrap_or_abort();
    assert!(summary.starts_with("child-large-summary:"));
    assert!(summary.ends_with('…'));
    assert!(!summary.contains("child-summary-tail"));
    assert_eq!(output["child_summary"]["kind"], json!("result"));
    assert_eq!(output["child_summary"]["truncated"], json!(true));
    assert_eq!(output["child_summary"]["max_chars"], json!(1200));
    assert_eq!(output["child_summary"]["summary"], json!(summary));
    assert!(output["child_session_id"].as_str().is_some());
}

fn assert_next_action_tool(output: &Value, tool: &str) {
    let actions = output["next_actions"].as_array().unwrap_or_abort();
    assert!(
        actions.iter().any(|action| action["tool"] == json!(tool)
            && action["parameters"]["session"] == output["child_session_id"]),
        "missing next action for {tool}: {actions:?}"
    );
}
