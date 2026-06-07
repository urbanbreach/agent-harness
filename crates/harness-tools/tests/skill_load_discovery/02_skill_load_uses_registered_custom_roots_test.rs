#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_load_uses_registered_custom_roots_and_permission_precedence() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let app = repo.join("packages/app");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&app).expect("app dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        project_roots: vec![PathBuf::from(".custom/skills")],
        global_roots: vec![home.join(".company/skills")],
        urls: Vec::new(),
        disabled: Vec::new(),
        walk_to_git_root: false,
        permissions: std::collections::BTreeMap::from([
            ("*".to_string(), PermissionMode::Allow),
            ("custom-*".to_string(), PermissionMode::Allow),
            ("custom-secret".to_string(), PermissionMode::Ask),
        ]),
    });

    write_skill(
        &app.join(".custom/skills"),
        "custom-visible",
        "Custom visible description",
        "Custom visible body",
    );
    write_skill(
        &app.join(".custom/skills"),
        "custom-secret",
        "Custom secret description",
        "Custom secret body",
    );
    write_skill(
        &repo.join(".custom/skills"),
        "custom-repo",
        "Repo-only description",
        "Repo-only body",
    );
    write_skill(
        &home.join(".company/skills"),
        "global-visible",
        "Global visible description",
        "Global visible body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");

    let visible = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-visible"),
            json!({"name": "custom-visible"}),
        )
        .await
        .expect("custom-visible skill");
    assert!(visible.display_text.contains("Custom visible description"));
    assert_eq!(
        visible
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(app
            .join(".custom/skills/custom-visible/SKILL.md")
            .display()
            .to_string()))
    );

    let agent_profiles = BTreeMap::from([("deep".to_string(), worker_profile("deep", &["skill"]))]);
    let (handle, run, worker_id) = spawn_worker_run(&app, "deep", agent_profiles).await;
    let gated_task = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    worker_actor(&worker_id),
                    Some("deep".to_string()),
                    "skill",
                    json!({"name": "custom-secret"}),
                )
                .await
        })
    };
    let permission_id =
        wait_for_question_permission(&run.events_path, None, Duration::from_secs(5)).await;
    handle
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["Yes"]]"#.to_string()),
        )
        .await
        .expect("approve custom-secret skill");
    let gated = gated_task
        .await
        .expect("join gated skill task")
        .expect("exact permission override should load after approval");
    assert!(gated.display_text.contains("Custom secret description"));
    handle.stop_run().await.expect("stop custom roots run");

    let repo_hidden = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-repo-hidden"),
            json!({"name": "custom-repo"}),
        )
        .await
        .expect_err("walk_to_git_root=false should skip repo-root skills from app cwd");
    assert!(repo_hidden
        .to_string()
        .contains("Skill \"custom-repo\" not found"));

    let global_visible = skill_tool
        .call(
            tool_context(&app, "toolcall-custom-global"),
            json!({"name": "global-visible"}),
        )
        .await
        .expect("global-visible skill");
    assert!(global_visible
        .display_text
        .contains("Global visible description"));
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_load_ask_permissions_use_question_approval_flow() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(&repo).expect("repo dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![home.join(".config/agent-harness/skills")],
        permissions: BTreeMap::from([
            ("*".to_string(), PermissionMode::Allow),
            ("experimental-*".to_string(), PermissionMode::Ask),
        ]),
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".agent-harness/skills"),
        "experimental-preview",
        "Ask description",
        "Ask body",
    );

    let agent_profiles = BTreeMap::from([("deep".to_string(), worker_profile("deep", &["skill"]))]);
    let (handle, run, worker_id) = spawn_worker_run(&repo, "deep", agent_profiles).await;

    let skill_task = {
        let handle = handle.clone();
        let worker_id = worker_id.clone();
        tokio::spawn(async move {
            handle
                .execute_agent_tool_call(
                    worker_actor(&worker_id),
                    Some("deep".to_string()),
                    "skill",
                    json!({"name": "experimental-preview"}),
                )
                .await
        })
    };
    let permission_id =
        wait_for_question_permission(&run.events_path, None, Duration::from_secs(5)).await;
    handle
        .resolve_permission(
            permission_id,
            PermissionDecision::Allow,
            Some(r#"[["Yes"]]"#.to_string()),
        )
        .await
        .expect("approve skill tool");
    let skill = skill_task
        .await
        .expect("join skill tool")
        .expect("skill tool result");

    assert!(skill.display_text.contains("Ask description"));
    assert!(skill.display_text.contains("Ask body"));
    assert!(skill.display_text.contains("# Skill: experimental-preview"));

    handle.stop_run().await.expect("stop run");
}
