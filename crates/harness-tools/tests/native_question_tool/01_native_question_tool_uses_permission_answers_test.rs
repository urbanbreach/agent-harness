use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn native_question_tool_uses_permission_answers() {
    let workspace = setup_workspace_fixture();
    let session_dir = workspace.temp_dir().join("sessions");
    let workspace_root = workspace.workspace();

    let coordinator = spawn_question_coordinator(session_dir, 1_000);
    let run = coordinator
        .start_run("native_question_success", workspace_root)
        .await
        .unwrap_or_abort();

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        run.run_id.as_str(),
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
        .unwrap_or_abort();

    let result = tool_task
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert_eq!(
        result.display_text,
        "User has answered your questions: \"Pick one\"=\"Unanswered\", \"Pick many\"=\"Beta, custom\". You can now continue with the user's answers in mind."
    );

    let structured = result.structured_json.unwrap_or_abort();
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
        .unwrap_or_abort();
    let question_state: Value =
        serde_json::from_slice(&fs::read(state_path).unwrap_or_abort())
            .unwrap_or_abort();
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

    coordinator.stop_run().await.unwrap_or_abort();
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
        .unwrap_or_abort();

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        run.run_id.as_str(),
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
        .unwrap_or_abort();

    let result = tool_task
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert!(result
        .display_text
        .contains("\"Which tool surface should be exercised next?\"=\"bash\""));

    let structured = result.structured_json.unwrap_or_abort();
    assert_eq!(structured.get("answers"), Some(&json!([["bash"]])));

    let state_path = structured
        .get("state_path")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    let question_state: Value =
        serde_json::from_slice(&fs::read(state_path).unwrap_or_abort())
            .unwrap_or_abort();
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

    coordinator.stop_run().await.unwrap_or_abort();
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
        .unwrap_or_abort();

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        run.run_id.as_str(),
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
        .unwrap_or_abort();

    let result = tool_task
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert!(result
        .display_text
        .contains("\"Choose the final stress-test summary level\"=\"detailed\""));

    let structured = result.structured_json.unwrap_or_abort();
    assert_eq!(structured.get("answers"), Some(&json!([["detailed"]])));

    let state_path = structured
        .get("state_path")
        .and_then(Value::as_str)
        .unwrap_or_abort();
    let question_state: Value =
        serde_json::from_slice(&fs::read(state_path).unwrap_or_abort())
            .unwrap_or_abort();
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

    coordinator.stop_run().await.unwrap_or_abort();
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
        .unwrap_or_abort();

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        run.run_id.as_str(),
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
        .unwrap_or_abort();

    let result = tool_task
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert!(result
        .display_text
        .contains("\"Pick the validation surface\"=\"read\""));

    coordinator.stop_run().await.unwrap_or_abort();
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
        .unwrap_or_abort();

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
        run.run_id.as_str(),
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
        .unwrap_or_abort();

    let result = tool_task
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert!(result.display_text.contains("\"Acknowledge that this question tool is reachable and return a one-line status.\"=\"reachable\""));

    let structured = result.structured_json.unwrap_or_abort();
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

    coordinator.stop_run().await.unwrap_or_abort();
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
        .unwrap_or_abort();

    let tool_task = spawn_question_tool_call(
        coordinator.clone(),
        run.run_id.as_str(),
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

    tokio::task::yield_now().await;
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
        .unwrap_or_abort();

    let result = tool_task
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    assert!(result
        .display_text
        .contains("\"Wait for a human answer\"=\"done\""));

    coordinator.stop_run().await.unwrap_or_abort();
}
