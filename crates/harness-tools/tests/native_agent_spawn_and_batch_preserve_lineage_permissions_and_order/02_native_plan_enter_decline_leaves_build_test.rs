#[tokio::test]
async fn native_plan_enter_decline_leaves_build_agent_active_without_spawning_plan() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry_with_question_answers(
        ShellAllowlist::default(),
        vec![vec!["No".to_string()]],
    ));
    config.agent_profiles = BTreeMap::from([
        (
            "build".to_string(),
            named_worker_profile("build", &["plan_enter"]),
        ),
        (
            "plan".to_string(),
            named_worker_profile("plan", &["plan_exit"]),
        ),
    ]);

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_enter_decline", &workspace)
        .await
        .expect("start run");
    let build_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "build", None)
        .await
        .expect("spawn build");

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&build_agent_id),
            Some("build".to_string()),
            "plan_enter",
            json!({"goal": "implement parity"}),
        )
        .await
        .expect("request plan_enter");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("plan_enter structured output");
    assert_eq!(output["agent"], "build");
    assert_eq!(output["approved"], false);
    assert_eq!(output["goal"], "implement parity");
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "plan"
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::UserMessageSubmitted(payload)
            if payload.text.contains("Your operational mode has changed from build to plan")
    )));
}
#[tokio::test]
async fn plan_profile_can_spawn_explore_but_bash_is_permission_denied() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let session_dir = workspace.join("sessions");
    fs::create_dir_all(&session_dir).expect("session dir");

    let mut config = CoordinatorConfig::new(session_dir);
    config.permission_policy = plan_mode_permission_policy();
    config.tool_registry = Arc::new(coordinator_registry(ShellAllowlist::default()));
    config.agent_profiles = plan_task_profiles();

    let handle = spawn_coordinator(
        config,
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle
        .start_run("native_plan_task_boundary", &workspace)
        .await
        .expect("start run");
    let plan_agent_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "plan", None)
        .await
        .expect("spawn plan");

    let denied = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "bash",
            json!({"command": "touch outside-plan", "description": "try mutating bash"}),
        )
        .await
        .expect_err("plan profile mutating bash must be permission denied");
    match denied {
        CoordinatorError::PermissionDenied(_) => {}
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PermissionResolved(data)
            if data.decision == EventPermissionDecision::Deny
                && data.reason.as_deref().is_some_and(|reason|
                    reason.contains("read-only inspection commands"))
    )));

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "subagent_type": "explore",
                "description": "Explore child",
                "prompt": "Inspect the fixture",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("plan profile can call task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("task structured output");
    assert_eq!(output["profile"], json!("explore"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == child_session_id && payload.profile == "explore"
    )));

    let denied_general = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "subagent_type": "general",
                "description": "General child",
                "prompt": "Try write-capable delegation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("plan profile can request task policy denial");
    wait_for_tool_call_finish(&run.events_path, &denied_general).await;

    let events = read_events(&run.events_path);
    let denied_finished = find_finished(&events, &denied_general);
    assert_eq!(denied_finished.status, ToolCallStatus::Failed);
    assert!(denied_finished
        .output_summary
        .as_deref()
        .unwrap_or_default()
        .contains("Plan mode may only delegate to the read-only `explore` profile"));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "general"
    )));

    let denied_custom = handle
        .request_tool_call(
            worker_actor(&plan_agent_id),
            Some("plan".to_string()),
            "task",
            json!({
                "subagent_type": "custom_writer",
                "description": "Custom child",
                "prompt": "Try user-defined write-capable delegation",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("plan profile can request custom task policy denial");
    wait_for_tool_call_finish(&run.events_path, &denied_custom).await;

    let events = read_events(&run.events_path);
    let denied_custom_finished = find_finished(&events, &denied_custom);
    assert_eq!(denied_custom_finished.status, ToolCallStatus::Failed);
    assert!(denied_custom_finished
        .output_summary
        .as_deref()
        .unwrap_or_default()
        .contains("Plan mode may only delegate to the read-only `explore` profile"));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "custom_writer"
    )));
}
#[tokio::test]
async fn task_subagent_type_selects_explore_and_general_profiles() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    for profile in ["explore", "general"] {
        let tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "subagent_type": profile,
                    "description": format!("{profile} child"),
                    "prompt": "Return a short answer",
                    "run_in_background": true,
                    "load_skills": []
                }),
            )
            .await
            .expect("request task");
        wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

        let events = read_events(&run.events_path);
        let finished = find_finished(&events, &tool_call_id);
        assert_eq!(finished.status, ToolCallStatus::Succeeded);
        let output = finished.output_json.expect("task structured output");
        assert_eq!(output["profile"], json!(profile));
        assert_eq!(output["runtime"]["profile"], json!(profile));
        assert_eq!(output["runtime"]["category"], json!(profile));
        assert_eq!(output["effective_model_ref"], json!("default:deep"));
        assert_eq!(output["can_redelegate"], json!(false));
        assert_eq!(output["has_background_output"], json!(false));
        assert!(output["child_toolset"].is_array());
        assert!(output["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action["action"] == json!("cancel")
                && action["tool"] == json!("background_cancel")));
        assert!(output["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action["action"] == json!("cancel_compat")
                && action["tool"] == json!("background_output")));
        let child_session_id = output["child_session_id"]
            .as_str()
            .expect("child session id");
        assert!(events.iter().any(|event| matches!(
            &event.payload,
            EventV1::AgentSpawned(payload)
                if payload.agent_id == child_session_id && payload.profile == profile
        )));
    }
}
#[tokio::test]
async fn task_subagent_type_wins_when_category_hint_is_also_present() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "quick",
                "subagent_type": "general",
                "description": "Direct subagent with category hint",
                "prompt": "Return a short answer",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("task structured output");
    assert_eq!(output["profile"], json!("general"));
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == child_session_id && payload.profile == "general"
    )));
}
#[tokio::test]
async fn worker_without_task_tool_cannot_redelegate() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, _worker_id) = spawn_run(&workspace).await;
    let general_id = handle
        .spawn_agent_idle(anonymous_supervisor_actor(), "general", None)
        .await
        .expect("spawn general worker");

    let denied = handle
        .request_tool_call(
            worker_actor(&general_id),
            Some("general".to_string()),
            "task",
            json!({
                "subagent_type": "general",
                "description": "Unauthorized redelegation",
                "prompt": "Try to spawn another child",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect_err("worker without task in its toolset must be denied");
    match denied {
        CoordinatorError::PolicyViolation(message) => {
            assert!(message.contains("not in worker toolset"));
        }
        other => panic!("expected worker toolset policy violation, got {other:?}"),
    }

    let events = read_events(&run.events_path);
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PolicyViolationDetected(payload)
            if payload.policy == "tool_not_in_toolset"
                && payload.detail.contains("task")
    )));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.parent_agent_id.as_deref() == Some(general_id.as_str())
    )));
}
#[tokio::test]
async fn task_category_without_matching_profile_falls_back_to_general() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "quick",
                "description": "Category fallback child",
                "prompt": "Return a short answer",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.expect("task structured output");
    assert_eq!(output["profile"], json!("general"));
    assert_eq!(output["route"]["requested_category"], json!("quick"));
    assert_eq!(output["route"]["resolved_profile"], json!("general"));
    assert_eq!(output["route"]["profile_id"], json!("general"));
    assert_eq!(output["route"]["role"], json!("subagent"));
    assert_eq!(output["route"]["hidden"], json!(false));
    assert_eq!(
        output["route"]["prompt"],
        json!({
            "source": "runtime_profile",
            "status": "resolved_by_coordinator",
            "profile": "general"
        })
    );
    assert_eq!(
        output["route"]["model"]["model_ref"],
        output["runtime"]["model_ref"]
    );
    assert_eq!(output["route"]["toolset"], output["runtime"]["toolset"]);
    assert_eq!(
        output["route"]["permission_posture"]["task"],
        json!("deny_by_toolset")
    );
    assert_eq!(output["route"]["loaded_skills"], json!([]));
    assert_eq!(output["route"]["fallback_chain"], json!(["quick", "general"]));
    assert_eq!(
        output["route"]["category_fallback_chain"],
        json!(["quick", "general"])
    );
    assert_eq!(output["route"]["fallback"]["applied"], json!(true));
    assert_eq!(
        output["route"]["fallback"]["fallback_profile"],
        json!("general")
    );
    assert_eq!(
        output["route"]["fallback"]["policy_source"],
        json!("harness_core::coord::task_category_fallback_profile")
    );
    let child_session_id = output["child_session_id"]
        .as_str()
        .expect("child session id");
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload)
            if payload.agent_id == child_session_id
                && payload.profile == "general"
    )));
}
#[tokio::test]
async fn background_output_retrieves_completed_child_result_by_request_id() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "category": "deep",
                "description": "Background child",
                "prompt": "Return a concise completed result",
                "run_in_background": true,
                "load_skills": []
            }),
        )
        .await
        .expect("request task");
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let task_events = read_events(&run.events_path);
    let task_finished = find_finished(&task_events, &task_tool_call_id);
    let task_output = task_finished.output_json.expect("task structured output");
    let request_id = task_output["child_request_id"]
        .as_str()
        .expect("child request id")
        .to_string();
    wait_for_request_terminal(&run.events_path, &request_id).await;

    let output_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "background_output",
            json!({
                "request_id": request_id,
                "block": true,
                "timeout_ms": 1
            }),
        )
        .await
        .expect("request background output");
    wait_for_tool_call_finish(&run.events_path, &output_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &output_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished
        .output_json
        .expect("background output structured json");
    assert_eq!(output["request_id"], json!(request_id));
    assert_eq!(output["status"], json!("completed"));
    assert_eq!(output["terminal"], json!(true));
    assert_eq!(output["timed_out"], json!(false));
    assert!(output["result_summary"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(output["runtime"]["profile"], json!("deep"));
    assert_eq!(output["runtime"]["model_ref"], json!("default:deep"));
    assert_eq!(output["runtime"]["can_redelegate"], json!(true));
    assert!(output["next_actions"]
        .as_array()
        .expect("next actions")
        .iter()
        .any(|action| action["action"] == json!("check_status")));
}
