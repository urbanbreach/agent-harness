use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::config::PermissionMode;
use harness_core::coord::{spawn_coordinator, CoordinatorConfig, CoordinatorHandle};
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::{PermissionDecision, PermissionPolicy};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::{ToolContext, ToolError};
use harness_tools::coordinator_registry;
use serde_json::{json, Value};
use tokio::time::{sleep, timeout, Duration, Instant};

fn actor() -> EventActor {
    EventActor::new(ActorKind::Worker, Some("agent-worker".to_string()))
}

fn read_events(path: &Path) -> Vec<EventEnvelopeV1> {
    fs::read_to_string(path)
        .expect("read events")
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect()
}

async fn wait_for_question_permission(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(permission_id) =
            read_events(path)
                .into_iter()
                .find_map(|event| match event.payload {
                    EventV1::PermissionRequested(data) if data.kind == "question" => {
                        Some(data.permission_id)
                    }
                    _ => None,
                })
        {
            return permission_id;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for question permission"
        );
        sleep(Duration::from_millis(20)).await;
    }
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
        actor: actor(),
        category: Some("deep".to_string()),
        plan_mode: false,
        plan_exit_target_profile: None,
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn spawn_question_coordinator(session_dir: PathBuf, ask_timeout_ms: u64) -> CoordinatorHandle {
    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
    .with_ask_timeout_ms(ask_timeout_ms);
    spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    )
}

#[tokio::test]
async fn native_question_tool_uses_permission_answers() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path().join("sessions");
    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).expect("workspace");

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_success", &workspace_root)
        .await
        .expect("start run");

    let question_tool = coordinator_registry(Default::default())
        .get("user.question")
        .expect("user.question tool");
    let tool_call_id = "native-question-success";
    let tool_task = tokio::spawn({
        let question_tool = question_tool.clone();
        let context = question_tool_context(
            coordinator.clone(),
            &run.run_id,
            &workspace_root,
            &run.artifacts_dir,
            tool_call_id,
        );
        async move {
            question_tool
                .call(
                    context,
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
                )
                .await
        }
    });

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
async fn native_question_tool_rejects_or_times_out_cleanly() {
    let reject_temp_dir = tempfile::tempdir().expect("tempdir");
    let reject_workspace_root = reject_temp_dir.path().join("workspace");
    fs::create_dir_all(&reject_workspace_root).expect("workspace");
    let reject_coordinator =
        spawn_question_coordinator(reject_temp_dir.path().join("sessions"), 1_000);
    let reject_run = reject_coordinator
        .start_run("native_question_reject", &reject_workspace_root)
        .await
        .expect("start reject run");

    let question_tool = coordinator_registry(Default::default())
        .get("user.question")
        .expect("user.question tool");
    let reject_task = tokio::spawn({
        let question_tool = question_tool.clone();
        let context = question_tool_context(
            reject_coordinator.clone(),
            &reject_run.run_id,
            &reject_workspace_root,
            &reject_run.artifacts_dir,
            "native-question-reject",
        );
        async move {
            question_tool
                .call(
                    context,
                    json!({
                        "questions": [{
                            "question": "Pick one",
                            "header": "Choice",
                            "options": [{"label": "A", "description": "Option A"}]
                        }]
                    }),
                )
                .await
        }
    });

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

    let timeout_temp_dir = tempfile::tempdir().expect("tempdir");
    let timeout_workspace_root = timeout_temp_dir.path().join("workspace");
    fs::create_dir_all(&timeout_workspace_root).expect("workspace");
    let timeout_coordinator =
        spawn_question_coordinator(timeout_temp_dir.path().join("sessions"), 25);
    let timeout_run = timeout_coordinator
        .start_run("native_question_timeout", &timeout_workspace_root)
        .await
        .expect("start timeout run");

    let timeout_task = tokio::spawn({
        let question_tool = question_tool.clone();
        let context = question_tool_context(
            timeout_coordinator.clone(),
            &timeout_run.run_id,
            &timeout_workspace_root,
            &timeout_run.artifacts_dir,
            "native-question-timeout",
        );
        async move {
            question_tool
                .call(
                    context,
                    json!({
                        "questions": [{
                            "question": "Pick one",
                            "header": "Choice",
                            "options": [{"label": "A", "description": "Option A"}]
                        }]
                    }),
                )
                .await
        }
    });

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
