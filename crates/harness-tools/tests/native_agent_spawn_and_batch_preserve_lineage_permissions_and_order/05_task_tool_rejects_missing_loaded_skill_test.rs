use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn task_tool_rejects_missing_loaded_skill_before_child_spawn() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    // act
    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Missing child skill",
                "prompt": "Try to inspect the repo",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": ["definitely-missing-skill"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .unwrap_or_abort()
        .contains("Skill \"definitely-missing-skill\" not found"));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "general"
    )));
}
#[tokio::test]
async fn task_tool_rejects_unloadable_loaded_skills_before_child_spawn() {
    // arrange
    let _registry_lock = skills_registry_test_lock().lock().await;
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        disabled: vec!["skill:project:disabled-child".to_string()],
        ..SkillsConfig::default()
    });
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture(&workspace, "internal-secret");
    write_skill_fixture(&workspace, "disabled-child");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "malformed-child",
        "name: malformed-child\ndescription: Malformed child\ntool_permissions: edit",
        "Malformed body.",
    );

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    for (name, expected_status) in [
        ("internal-secret", "denied"),
        ("disabled-child", "disabled"),
        ("malformed-child", "malformed"),
    ] {
        let task_tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "description": format!("Unloadable child skill {name}"),
                    "prompt": "Try to inspect the repo",
                    "subagent_type": "general",
                    "run_in_background": false,
                    "load_skills": [name]
                }),
            )
            .await
            .unwrap_or_abort();
        wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

        let events = read_events(&run.events_path);
        // act
        let finished = find_finished(&events, &task_tool_call_id);
        // assert
        assert_eq!(finished.status, ToolCallStatus::Failed);
        let summary = finished.output_summary.as_deref().unwrap_or_abort();
        assert!(summary.contains(name), "summary should name {name}: {summary}");
        assert!(
            summary.contains(expected_status),
            "summary should include {expected_status}: {summary}"
        );
        assert!(!events.iter().any(|event| matches!(
            &event.payload,
            EventV1::AgentSpawned(payload) if payload.profile == "general"
        )));
    }

    handle.stop_run().await.unwrap_or_abort();
}
#[cfg(unix)]
#[tokio::test]
async fn task_tool_rejects_symlinked_loaded_skill_before_child_spawn() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let outside_skill = temp_dir.path().join("outside-skill");
    fs::create_dir_all(&outside_skill).unwrap_or_abort();
    fs::write(
        outside_skill.join("SKILL.md"),
        "---\nname: evil\ndescription: Evil description\n---\n\nEvil body.\n",
    )
    .unwrap_or_abort();
    let skill_root = workspace.join(".agent-harness/skills");
    fs::create_dir_all(&skill_root).unwrap_or_abort();
    std::os::unix::fs::symlink(&outside_skill, skill_root.join("evil"))
        .unwrap_or_abort();

    let (handle, run, worker_id) = spawn_run(&workspace).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Symlink child skill",
                "prompt": "Try to load a symlinked skill",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": ["evil"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);

    assert_eq!(finished.status, ToolCallStatus::Failed);
    assert!(finished
        .output_summary
        .as_deref()
        .unwrap_or_abort()
        .contains("Skill \"evil\" not found"));
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "general"
    )));
}
#[tokio::test]
async fn task_tool_injects_loaded_skill_content_into_child_prompt() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let skill_dir = workspace.join(".agent-harness/skills/task-skill");
    fs::create_dir_all(&skill_dir).unwrap_or_abort();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: task-skill\ndescription: Task skill description\n---\n\nTask skill body marker.\n",
    )
    .unwrap_or_abort();
    let provider = Arc::new(TaskCallingProvider::default());
    let provider_clone = Arc::clone(&provider);
    let (handle, run, worker_id) = spawn_run_with_provider(&workspace, provider_clone).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Skill child",
                "prompt": "Use the injected skill",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": ["task-skill"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(output["loaded_skills"][0]["name"], json!("task-skill"));
    assert_eq!(
        output["loaded_skills"][0]["stable_id"],
        json!("skill:project:task-skill")
    );
    assert_eq!(output["loaded_skills"][0]["status"], json!("loadable"));
    assert_eq!(output["loaded_skills"][0]["source_scope"], json!("project"));
    assert_eq!(output["loaded_skills"][0]["body_loaded"], json!(false));
    assert_eq!(
        output["route"]["loaded_skills"][0]["stable_id"],
        json!("skill:project:task-skill")
    );
    assert!(!output.to_string().contains("Task skill body marker."));
    assert_eq!(output["load_skills"], json!(["task-skill"]));
    assert!(output["next_actions"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .any(|action| action["action"] == json!("continue_task")
            && action["tool"] == json!("task")
            && action["parameters"]["run_in_background"] == json!(false)
            && action["parameters"]["load_skills"] == json!([])));

    let requests = provider.requests().await;
    assert_eq!(
        requests.len(),
        1,
        "expected only the child provider request"
    );
    let prompt = requests[0]
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(prompt.contains("<skill_content name=\"task-skill\">"));
    assert!(prompt.contains("Task skill description"));
    assert!(prompt.contains("Task skill body marker."));
    assert!(prompt.contains("Base directory for this skill: file://"));
    assert!(prompt.contains("Use the injected skill"));
}

#[tokio::test]
async fn task_tool_injects_shipped_builtin_skill_bodies_into_child_prompt() {
    // arrange
    let _registry_lock = skills_registry_test_lock().lock().await;
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: Vec::new(),
        ..SkillsConfig::default()
    });
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let shipped_skill_root = repo_root().join(".agent-harness/skills");
    let workspace_skill_root = workspace.join(".agent-harness/skills");
    for name in ["git-master", "review-work", "frontend-ui-ux"] {
        let skill_dir = workspace_skill_root.join(name);
        fs::create_dir_all(&skill_dir).unwrap_or_abort();
        fs::copy(
            shipped_skill_root.join(name).join("SKILL.md"),
            skill_dir.join("SKILL.md"),
        )
        .unwrap_or_else(|err| panic!("copy shipped built-in skill {name}: {err}"));
    }
    let provider = Arc::new(TaskCallingProvider::default());
    let provider_clone = Arc::clone(&provider);
    let (handle, run, worker_id) = spawn_run_with_provider(&workspace, provider_clone).await;

    // act
    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Shipped built-in skill child",
                "prompt": "Use the shipped built-in skills",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": ["git-master", "review-work", "frontend-ui-ux"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    // assert
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    let loaded_skills = output["loaded_skills"]
        .as_array()
        .unwrap_or_abort();
    assert_eq!(loaded_skills.len(), 3);
    for name in ["git-master", "review-work", "frontend-ui-ux"] {
        assert!(loaded_skills.iter().any(|skill| {
            skill["name"] == json!(name)
                && skill["stable_id"] == json!(format!("skill:project:{name}"))
                && skill["status"] == json!("loadable")
                && skill["source_scope"] == json!("project")
                && skill["body_loaded"] == json!(false)
        }));
    }

    let requests = provider.requests().await;
    assert_eq!(requests.len(), 1, "expected only the child provider request");
    let prompt = requests[0]
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for (name, marker) in [
        ("git-master", "Use git with atomic, reviewable intent"),
        ("review-work", "Run a structured post-implementation review"),
        ("frontend-ui-ux", "Improve visible UI surfaces"),
    ] {
        assert!(prompt.contains(&format!("<skill_content name=\"{name}\">")));
        assert!(prompt.contains(marker), "prompt missing shipped marker for {name}");
    }
    assert!(prompt.contains("Use the shipped built-in skills"));
}

#[tokio::test]
async fn task_tool_rejects_disabled_shipped_builtin_skill_before_child_spawn() {
    let _registry_lock = skills_registry_test_lock().lock().await;
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: Vec::new(),
        disabled: vec!["skill:project:git-master".to_string()],
        ..SkillsConfig::default()
    });
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let skill_dir = workspace.join(".agent-harness/skills/git-master");
    fs::create_dir_all(&skill_dir).unwrap_or_abort();
    fs::copy(
        repo_root()
            .join(".agent-harness/skills/git-master")
            .join("SKILL.md"),
        skill_dir.join("SKILL.md"),
    )
    .unwrap_or_abort();
    let (handle, run, worker_id) = spawn_run(&workspace).await;

    // act
    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Disabled built-in child skill",
                "prompt": "Try to use disabled git-master",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": ["git-master"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    handle.stop_run().await.unwrap_or_abort();
    let events = read_events(&run.events_path);
    let finished = find_finished(&events, &task_tool_call_id);
    assert_eq!(finished.status, ToolCallStatus::Failed);
    let summary = finished.output_summary.as_deref().unwrap_or_abort();
    assert!(summary.contains("git-master"), "summary should name git-master: {summary}");
    assert!(
        summary.contains("disabled by skills.disabled"),
        "summary should explain stable-id disablement: {summary}"
    );
    assert!(!events.iter().any(|event| matches!(
        &event.payload,
        EventV1::AgentSpawned(payload) if payload.profile == "general"
    )));
}

#[tokio::test]
async fn task_tool_preserves_requested_skill_order_and_deduplicates_loaded_context() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "alpha-skill",
        "name: alpha-skill\ndescription: Alpha skill description",
        "Alpha body marker.",
    );
    write_skill_fixture_with_frontmatter(
        &workspace,
        "beta-skill",
        "name: beta-skill\ndescription: Beta skill description",
        "Beta body marker.",
    );
    let provider = Arc::new(TaskCallingProvider::default());
    let provider_clone = Arc::clone(&provider);
    let (handle, run, worker_id) = spawn_run_with_provider(&workspace, provider_clone).await;

    let task_tool_call_id = handle
        .request_tool_call(
            worker_actor(&worker_id),
            Some("deep".to_string()),
            "task",
            json!({
                "description": "Ordered skill child",
                "prompt": "Use ordered skills",
                "subagent_type": "general",
                "run_in_background": false,
                "load_skills": ["alpha-skill", "beta-skill", "alpha-skill"]
            }),
        )
        .await
        .unwrap_or_abort();
    wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;

    let events = read_events(&run.events_path);
    // act
    let finished = find_finished(&events, &task_tool_call_id);
    // assert
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let output = finished.output_json.unwrap_or_abort();
    assert_eq!(
        output["load_skills"],
        json!(["alpha-skill", "beta-skill", "alpha-skill"])
    );
    assert_eq!(output["loaded_skills"].as_array().map(Vec::len), Some(2));
    assert_eq!(output["loaded_skills"][0]["name"], json!("alpha-skill"));
    assert_eq!(output["loaded_skills"][1]["name"], json!("beta-skill"));
    assert_eq!(output["route"]["loaded_skills"][0]["name"], json!("alpha-skill"));
    assert_eq!(output["route"]["loaded_skills"][1]["name"], json!("beta-skill"));
    assert!(!output.to_string().contains("Alpha body marker."));
    assert!(!output.to_string().contains("Beta body marker."));

    let requests = provider.requests().await;
    assert_eq!(requests.len(), 1, "expected only the child provider request");
    let prompt = requests[0]
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let alpha_position = prompt
        .find("<skill_content name=\"alpha-skill\">")
        .unwrap_or_abort();
    let beta_position = prompt
        .find("<skill_content name=\"beta-skill\">")
        .unwrap_or_abort();
    let task_position = prompt.find("Use ordered skills").unwrap_or_abort();
    assert!(alpha_position < beta_position);
    assert!(beta_position < task_position);
    assert_eq!(
        prompt.matches("<skill_content name=\"alpha-skill\">").count(),
        1
    );
}
