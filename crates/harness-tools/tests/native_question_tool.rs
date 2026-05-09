use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::EventV1;
use harness_core::perm::PermissionDecision;
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{Tool, ToolContext, ToolError, ToolResult};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};

mod common;

use common::{
    allow_all_permission_policy, read_events, setup_workspace_fixture,
    wait_for_question_permission as wait_for_question_permission_event, worker_actor,
};

async fn wait_for_question_permission(path: &Path) -> String {
    wait_for_question_permission_event(path, None, Duration::from_secs(5)).await
}

fn question_tool_context(
    coordinator: CoordinatorHandle,
    run_id: &str,
    workspace_root: &Path,
    artifacts_dir: &Path,
    tool_call_id: &str,
) -> ToolContext {
    ToolContext {
        run_id: run_id.to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: artifacts_dir.to_path_buf(),
        actor: worker_actor("agent-worker"),
        category: Some("deep".to_string()),
        tool_call_id: tool_call_id.to_string(),
        current_model_ref: None,
        current_model_settings: None,
        coordinator,
    }
}

fn spawn_question_coordinator(session_dir: PathBuf, ask_timeout_ms: u64) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = allow_all_permission_policy().with_ask_timeout_ms(ask_timeout_ms);
    spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

fn question_tool() -> Arc<dyn Tool> {
    coordinator_registry(Default::default())
        .get("question")
        .expect("question tool")
}

fn spawn_question_tool_call(
    coordinator: CoordinatorHandle,
    run_id: &str,
    workspace_root: &Path,
    artifacts_dir: &Path,
    tool_call_id: &str,
    args: Value,
) -> tokio::task::JoinHandle<Result<ToolResult, ToolError>> {
    let question_tool = question_tool();
    let context = question_tool_context(
        coordinator,
        run_id,
        workspace_root,
        artifacts_dir,
        tool_call_id,
    );
    tokio::spawn(async move { question_tool.call(context, args).await })
}

#[tokio::test]
async fn native_question_tool_uses_permission_answers() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_success", workspace_root)
        .await
        .expect("start run");

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        &run.run_id,
        workspace_root,
        &run.artifacts_dir,
        "native-question-success",
        json!({
            "questions": [
                {
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [
                        {"label": "Yes", "description": "Choose yes"},
                        {"label": "No", "description": "Choose no"}
                    ]
                },
                {
                    "question": "Pick many",
                    "header": "Multi",
                    "multiple": true,
                    "options": [
                        {"label": "Alpha", "description": "Choose alpha"},
                        {"label": "Beta", "description": "Choose beta"}
                    ]
                }
            ]
        }),
    );

    let permission_id = wait_for_question_permission(&run.events_path).await;
    coordinator
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["   "],["beta","custom"]]"#.to_string()),
        )
        .await
        .expect("resolve question permission");

    let result = tool_task
        .await
        .expect("join question tool task")
        .expect("question tool result");
    assert_eq!(
        result.display_text,
        "User has answered your questions: \"Pick one\"=\"Unanswered\", \"Pick many\"=\"Beta, custom\". You can now continue with the user's answers in mind."
    );

    let structured = result.structured_json.expect("structured json");
    assert_eq!(
        structured.get("answers"),
        Some(&json!([[], ["Beta", "custom"]]))
    );
    assert_eq!(
        structured.get("output"),
        Some(&Value::String(result.display_text.clone()))
    );

    let state_path = structured
        .get("state_path")
        .and_then(Value::as_str)
        .expect("state_path in structured output");
    let question_state: Value =
        serde_json::from_slice(&fs::read(state_path).expect("read persisted question state"))
            .expect("parse persisted question state");
    assert_eq!(
        question_state,
        json!([
            {
                "question": "Pick one",
                "header": "Choice",
                "options": [
                    {"label": "Yes", "description": "Choose yes"},
                    {"label": "No", "description": "Choose no"}
                ],
                "multiple": Value::Null
            },
            {
                "question": "Pick many",
                "header": "Multi",
                "options": [
                    {"label": "Alpha", "description": "Choose alpha"},
                    {"label": "Beta", "description": "Choose beta"}
                ],
                "multiple": true
            }
        ])
    );

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn native_question_tool_accepts_string_option_shorthand() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_shorthand", workspace_root)
        .await
        .expect("start run");

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        &run.run_id,
        workspace_root,
        &run.artifacts_dir,
        "native-question-shorthand",
        json!({
            "questions": [
                {
                    "question": "Which tool surface should be exercised next?",
                    "required": true,
                    "options": ["bash", "pty", "task"]
                }
            ]
        }),
    );

    let permission_id = wait_for_question_permission(&run.events_path).await;
    coordinator
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["bash"]]"#.to_string()),
        )
        .await
        .expect("resolve question permission");

    let result = tool_task
        .await
        .expect("join question tool task")
        .expect("question tool result");
    assert!(result
        .display_text
        .contains("\"Which tool surface should be exercised next?\"=\"bash\""));

    let structured = result.structured_json.expect("structured json");
    assert_eq!(structured.get("answers"), Some(&json!([["bash"]])));

    let state_path = structured
        .get("state_path")
        .and_then(Value::as_str)
        .expect("state_path in structured output");
    let question_state: Value =
        serde_json::from_slice(&fs::read(state_path).expect("read persisted question state"))
            .expect("parse persisted question state");
    assert_eq!(
        question_state,
        json!([
            {
                "question": "Which tool surface should be exercised next?",
                "header": "Which tool surface should be exercised next?",
                "options": [
                    {"label": "bash", "description": "bash"},
                    {"label": "pty", "description": "pty"},
                    {"label": "task", "description": "task"}
                ],
                "multiple": Value::Null
            }
        ])
    );

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn native_question_tool_accepts_single_question_shape_and_legacy_fields() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_single_legacy", workspace_root)
        .await
        .expect("start run");

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        &run.run_id,
        workspace_root,
        &run.artifacts_dir,
        "native-question-single-legacy",
        json!({
            "id": "q1",
            "question": "Choose the final stress-test summary level",
            "header": "Harness stress test",
            "required": true,
            "choices": ["short", "medium", "detailed"]
        }),
    );

    let permission_id = wait_for_question_permission(&run.events_path).await;
    coordinator
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["detailed"]]"#.to_string()),
        )
        .await
        .expect("resolve question permission");

    let result = tool_task
        .await
        .expect("join question tool task")
        .expect("legacy question tool result");
    assert!(result
        .display_text
        .contains("\"Choose the final stress-test summary level\"=\"detailed\""));

    let structured = result.structured_json.expect("structured json");
    assert_eq!(structured.get("answers"), Some(&json!([["detailed"]])));

    let state_path = structured
        .get("state_path")
        .and_then(Value::as_str)
        .expect("state_path in structured output");
    let question_state: Value =
        serde_json::from_slice(&fs::read(state_path).expect("read persisted question state"))
            .expect("parse persisted question state");
    assert_eq!(
        question_state,
        json!([
            {
                "question": "Choose the final stress-test summary level",
                "header": "Harness stress test",
                "options": [
                    {"label": "short", "description": "short"},
                    {"label": "medium", "description": "medium"},
                    {"label": "detailed", "description": "detailed"}
                ],
                "multiple": Value::Null
            }
        ])
    );

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn native_question_tool_accepts_allow_freeform_legacy_field() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_allow_freeform_legacy", workspace_root)
        .await
        .expect("start run");

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        &run.run_id,
        workspace_root,
        &run.artifacts_dir,
        "native-question-allow-freeform-legacy",
        json!({
            "questions": [{
                "question": "Pick the validation surface",
                "options": ["read", "bash"],
                "allowFreeform": false
            }]
        }),
    );

    let permission_id = wait_for_question_permission(&run.events_path).await;
    coordinator
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["read"]]"#.to_string()),
        )
        .await
        .expect("resolve question permission");

    let result = tool_task
        .await
        .expect("join question tool task")
        .expect("allowFreeform legacy question tool result");
    assert!(result
        .display_text
        .contains("\"Pick the validation surface\"=\"read\""));

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn native_question_tool_accepts_text_prompt_compat_shape_and_schema_advertises_it() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_text_compat", workspace_root)
        .await
        .expect("start run");

    let question_tool = question_tool();
    let schema = question_tool.parameters_json_schema();
    assert_eq!(schema["type"], json!("object"));
    assert_eq!(schema["required"], json!(["questions"]));
    assert!(schema.to_string().contains("\"allowFreeform\""));
    assert!(schema.to_string().contains("\"type\""));
    assert!(schema["properties"]["questions"]["description"]
        .as_str()
        .is_some_and(|value| value.contains("top-level arrays and single-question payloads")));

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        &run.run_id,
        workspace_root,
        &run.artifacts_dir,
        "native-question-text-compat",
        json!({
            "questions": [{
                "id": "stress-sanity",
                "question": "Acknowledge that this question tool is reachable and return a one-line status.",
                "type": "text"
            }]
        }),
    );

    let permission_id = wait_for_question_permission(&run.events_path).await;
    coordinator
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["reachable"]]"#.to_string()),
        )
        .await
        .expect("resolve question permission");

    let result = tool_task
        .await
        .expect("join question tool task")
        .expect("text compat question tool result");
    assert!(result.display_text.contains("\"Acknowledge that this question tool is reachable and return a one-line status.\"=\"reachable\""));

    let structured = result.structured_json.expect("structured json");
    assert_eq!(structured.get("answers"), Some(&json!([["reachable"]])));
    assert_eq!(
        structured.get("questions"),
        Some(&json!([
            {
                "question": "Acknowledge that this question tool is reachable and return a one-line status.",
                "header": "Acknowledge that this question tool is reachable and return a one-line status.",
                "options": [],
                "multiple": Value::Null
            }
        ]))
    );

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn native_question_tool_waits_indefinitely_when_timeout_disabled() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 0);
    let run = coordinator
        .start_run("native_question_no_timeout", workspace_root)
        .await
        .expect("start run");

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        &run.run_id,
        workspace_root,
        &run.artifacts_dir,
        "native-question-no-timeout",
        json!({
            "questions": [{
                "question": "Wait for a human answer",
                "options": ["keep waiting", "done"]
            }]
        }),
    );

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        !tool_task.is_finished(),
        "question should still be pending with timeout disabled"
    );

    let permission_id = wait_for_question_permission(&run.events_path).await;
    coordinator
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["done"]]"#.to_string()),
        )
        .await
        .expect("resolve question permission");

    let result = tool_task
        .await
        .expect("join question tool task")
        .expect("question result with timeout disabled");
    assert!(result
        .display_text
        .contains("\"Wait for a human answer\"=\"done\""));

    coordinator.stop_run().await.expect("stop run");
}

#[tokio::test]
async fn native_question_tool_rejects_or_times_out_cleanly() {
    let reject_workspace = setup_workspace_fixture();
    let reject_workspace_root = reject_workspace.workspace();
    let reject_coordinator =
        spawn_question_coordinator(reject_workspace.temp_dir().join("sessions"), 1_000);
    let reject_run = reject_coordinator
        .start_run("native_question_reject", reject_workspace_root)
        .await
        .expect("start reject run");

    let reject_task = spawn_question_tool_call(
        reject_coordinator.clone(),
        &reject_run.run_id,
        reject_workspace_root,
        &reject_run.artifacts_dir,
        "native-question-reject",
        json!({
            "questions": [{
                "question": "Pick one",
                "header": "Choice",
                "options": [{"label": "A", "description": "Option A"}]
            }]
        }),
    );

    let reject_permission_id = wait_for_question_permission(&reject_run.events_path).await;
    reject_coordinator
        .resolve_permission(reject_permission_id.clone(), PermissionDecision::Deny, None)
        .await
        .expect("deny question permission");
    let reject_err = reject_task
        .await
        .expect("join reject task")
        .expect_err("denied question should fail");
    assert!(matches!(
        reject_err,
        ToolError::Execution(message) if message == "question rejected by user"
    ));
    assert!(read_events(&reject_run.events_path).iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.permission_id == reject_permission_id
                    && data.reason.is_none()
        )
    }));
    reject_coordinator
        .stop_run()
        .await
        .expect("stop reject run");

    let timeout_workspace = setup_workspace_fixture();
    let timeout_workspace_root = timeout_workspace.workspace();
    let timeout_coordinator =
        spawn_question_coordinator(timeout_workspace.temp_dir().join("sessions"), 25);
    let timeout_run = timeout_coordinator
        .start_run("native_question_timeout", timeout_workspace_root)
        .await
        .expect("start timeout run");

    let timeout_task = spawn_question_tool_call(
        timeout_coordinator.clone(),
        &timeout_run.run_id,
        timeout_workspace_root,
        &timeout_run.artifacts_dir,
        "native-question-timeout",
        json!({
            "questions": [{
                "question": "Pick one",
                "header": "Choice",
                "options": [{"label": "A", "description": "Option A"}]
            }]
        }),
    );

    let timeout_err = timeout(Duration::from_secs(2), timeout_task)
        .await
        .expect("question timeout should complete")
        .expect("join timeout task")
        .expect_err("timed out question should fail");
    assert!(matches!(
        timeout_err,
        ToolError::Execution(message) if message == "question timed out awaiting user input"
    ));
    assert!(read_events(&timeout_run.events_path).iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionResolved(data)
                if data.reason.as_deref() == Some("permission request timed out")
        )
    }));
    timeout_coordinator
        .stop_run()
        .await
        .expect("stop timeout run");
}
