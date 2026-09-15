use harness_tools::UnwrapOrAbort;
#[tokio::test]
async fn child_agent_toolset_boundary_is_enforced() {
    // arrange
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    write_skill_fixture_with_frontmatter(
        &workspace,
        "tool-claim-skill",
        "name: tool-claim-skill\ndescription: Claims tools but cannot grant them\nallowed_tools: task, edit",
        "Tool claim body.",
    );

    fs::write(workspace.join("fixture.txt"), "role boundary needle\n").unwrap_or_abort();
    let mut parent = worker_profile(&["task", "write"]);
    parent.permission_ruleset = harness_core::perm::from_profile_permissions(&ProfilePermissions {
        edit: Some(PermissionMode::Deny),
        ..ProfilePermissions::default()
    });
    let mut profiles = BTreeMap::from([("default".to_string(), parent)]);
    for name in ["explore", "librarian", "general"] {
        let mut profile = named_worker_profile(name, &["read", "grep", "batch", "write", "bash", "skill"]);
        profile.permission_ruleset =
            harness_core::perm::from_profile_permissions(&ProfilePermissions {
                edit: Some(if name == "general" {
                    PermissionMode::Allow
                } else {
                    PermissionMode::Deny
                }),
                shell: Some(PermissionMode::Deny),
                task: Some(PermissionMode::Deny),
                ..ProfilePermissions::default()
            });
        profiles.insert(name.to_string(), profile);
    }
    let (handle, run, worker_id) =
        spawn_run_with_provider_and_profiles(&workspace, Arc::new(StaticProvider), profiles).await;

    for role in ["explore", "librarian", "general"] {
        let task_tool_call_id = handle
            .request_tool_call(
                worker_actor(&worker_id),
                Some("deep".to_string()),
                "task",
                json!({
                    "description": "Restricted child",
                    "prompt": "Stay read-only",
                    "subagent_type": role,
                    "run_in_background": true,
                    "load_skills": ["tool-claim-skill"]
                }),
            )
            .await
            .unwrap_or_abort();

        wait_for_tool_call_finish(&run.events_path, &task_tool_call_id).await;
        let events = read_events(&run.events_path);
        let finished = find_finished(&events, &task_tool_call_id);
        // act
        let output = finished.output_json.unwrap_or_abort();
        // assert
        assert_eq!(
            output["route"]["loaded_skills"][0]["allowed_tools"],
            json!(["task", "edit"])
        );

        assert_eq!(output["can_redelegate"], json!(false));
        assert!(!output["route"]["toolset"]
            .as_array()
            .unwrap_or_abort()
            .iter()
            .any(|tool| tool == "task" || tool == "edit"));
        let child_session_id = output["child_session_id"].as_str().unwrap_or_abort();

        let skill = handle
            .request_tool_call(
                worker_actor(child_session_id),
                Some("default".to_string()),
                "skill",
                json!({"name": "tool-claim-skill"}),
            )
            .await
            .unwrap_or_abort();
        wait_for_tool_call_finish(&run.events_path, &skill).await;
        assert_eq!(
            find_finished(&read_events(&run.events_path), &skill).status,
            ToolCallStatus::Succeeded,
        );

        let write_args = json!({"filePath": format!("{role}.txt"), "content": "implemented"});
        let direct = handle
            .request_tool_call(
                worker_actor(child_session_id),
                Some("default".to_string()),
                "write",
                write_args.clone(),
            )
            .await;
        if role == "general" {
            let call = direct.unwrap_or_abort();
            wait_for_tool_call_finish(&run.events_path, &call).await;
            assert_eq!(
                find_finished(&read_events(&run.events_path), &call).status,
                ToolCallStatus::Succeeded
            );
            assert_eq!(
                fs::read_to_string(workspace.join("general.txt")).unwrap_or_abort(),
                "implemented"
            );
        } else {
            assert!(direct
                .unwrap_err()
                .to_string()
                .contains("not in worker toolset"));
            let bash_args = json!({"command": format!("touch {role}-shell.txt"), "workdir": ".", "description": "Attempt mutation"});
            let denied = handle
                .request_tool_call(
                    worker_actor(child_session_id),
                    Some("default".to_string()),
                    "bash",
                    bash_args.clone(),
                )
                .await
                .unwrap_err();
            assert!(denied.to_string().contains("not in worker toolset"));
            let batch = handle
                .request_tool_call(
                    worker_actor(child_session_id),
                    Some("default".to_string()),
                    "batch",
                    json!({"tool_calls": [
                        {"tool": "write", "parameters": write_args},
                        {"tool": "bash", "parameters": bash_args},
                        {"tool": "read", "parameters": {"filePath": "fixture.txt"}},
                        {"tool": "grep", "parameters": {"pattern": "needle", "path": "."}}
                    ]}),
                )
                .await
                .unwrap_or_abort();
            wait_for_tool_call_finish(&run.events_path, &batch).await;
            let finished = find_finished(&read_events(&run.events_path), &batch);
            let result = finished.output_json.unwrap_or_abort();
            assert_eq!(
                result["details"]
                    .as_array()
                    .unwrap_or_abort()
                    .iter()
                    .map(|detail| detail["success"].clone())
                    .collect::<Vec<_>>(),
                vec![json!(false), json!(false), json!(true), json!(true)],
                "{result}",
            );
            assert!(!workspace.join(format!("{role}.txt")).exists());
            assert!(!workspace.join(format!("{role}-shell.txt")).exists());
        }
        let denied = handle.request_tool_call(
        worker_actor(child_session_id), Some("default".to_string()), "task",
        json!({"prompt": "try child delegation", "run_in_background": false, "load_skills": []}),
    ).await.expect_err("child must not be able to redelegate");
        assert!(denied
            .to_string()
            .contains("tool `task` is not in worker toolset"));
    }
    handle.stop_run().await.unwrap_or_abort();
}
