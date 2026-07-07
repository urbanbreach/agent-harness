use harness_tools::UnwrapOrAbort;
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_discovery_uses_workspace_root_not_process_cwd() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let outside = temp_dir.path().join("outside-cwd");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(&outside).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_with_global_root(
        home.join(".config/agent-harness/skills"),
    ));

    write_skill(
        &repo.join(".agent-harness/skills"),
        "workspace-skill",
        "Workspace description",
        "Workspace body",
    );
    write_skill(
        &outside.join(".agent-harness/skills"),
        "cwd-only",
        "Cwd description",
        "Cwd body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let workspace_skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-workspace-root-skill"),
            json!({"name": "workspace-skill"}),
        )
        .await
        .unwrap_or_abort();
    assert!(workspace_skill
        .display_text
        .contains("Workspace description"));

    let cwd_only = skill_tool
        .call(
            tool_context(&repo, "toolcall-cwd-only-skill"),
            json!({"name": "cwd-only"}),
        )
        .await
        .expect_err("cwd skill should not be loaded from process cwd");
    assert!(cwd_only
        .to_string()
        .contains("Skill \"cwd-only\" not found"));
}
#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_discovery_rejects_symlinked_skill_directories() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let outside = temp_dir.path().join("outside-skill");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_with_global_root(
        home.join(".config/agent-harness/skills"),
    ));

    write_skill(
        temp_dir.path(),
        "outside-skill",
        "Outside description",
        "Outside body",
    );
    let skill_root = repo.join(".agent-harness/skills");
    fs::create_dir_all(&skill_root).unwrap_or_abort();
    std::os::unix::fs::symlink(&outside, skill_root.join("evil")).unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-symlink-skill"),
            json!({"name": "evil"}),
        )
        .await
        .expect_err("symlinked skill should be rejected");
    assert!(err.to_string().contains("Skill \"evil\" not found"));
}
#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_discovery_rejects_symlinked_project_skill_root() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let outside = temp_dir.path().join("outside-root");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_with_global_root(
        home.join(".config/agent-harness/skills"),
    ));

    write_skill(
        &outside,
        "evil-root-skill",
        "Outside root description",
        "Outside root body",
    );
    let agent_harness_dir = repo.join(".agent-harness");
    fs::create_dir_all(&agent_harness_dir).unwrap_or_abort();
    std::os::unix::fs::symlink(&outside, agent_harness_dir.join("skills"))
        .unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-symlink-root-skill"),
            json!({"name": "evil-root-skill"}),
        )
        .await
        .expect_err("symlinked project skill root should be ignored");
    assert!(err
        .to_string()
        .contains("Skill \"evil-root-skill\" not found"));
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_load_hides_denied_or_invalid_skills() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![home.join(".config/agent-harness/skills")],
        permissions: BTreeMap::from([
            ("*".to_string(), PermissionMode::Allow),
            ("experimental-*".to_string(), PermissionMode::Allow),
            ("internal-*".to_string(), PermissionMode::Deny),
        ]),
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".agent-harness/skills"),
        "visible-skill",
        "Visible description",
        "Visible body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "internal-secret",
        "Denied description",
        "Denied body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "experimental-preview",
        "Ask description",
        "Ask body",
    );
    write_invalid_skill(&repo.join(".agent-harness/skills"), "broken-skill");
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "broken-skill",
        "Global description",
        "Global body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let visible = skill_tool
        .call(
            tool_context(&repo, "toolcall-visible-skill"),
            json!({"name": "visible-skill"}),
        )
        .await
        .unwrap_or_abort();
    assert!(visible.display_text.contains("Visible description"));

    let denied = skill_tool
        .call(
            tool_context(&repo, "toolcall-denied-skill"),
            json!({"name": "internal-secret"}),
        )
        .await
        .expect_err("denied skill should be hidden");
    assert!(denied
        .to_string()
        .contains("Skill \"internal-secret\" not found"));

    let approved = skill_tool
        .call(
            tool_context(&repo, "toolcall-ask-skill"),
            json!({"name": "experimental-preview"}),
        )
        .await
        .unwrap_or_abort();
    assert!(approved.display_text.contains("Ask description"));
    assert!(approved.display_text.contains("Ask body"));

    let invalid = skill_tool
        .call(
            tool_context(&repo, "toolcall-invalid-skill"),
            json!({"name": "broken-skill"}),
        )
        .await
        .expect_err("invalid higher-precedence skill should hide lower-precedence skill");
    assert!(invalid
        .to_string()
        .contains("Skill \"broken-skill\" not found"));
}
#[test]
fn skill_permissions_hide_denied_and_reject_invalid_frontmatter() {
    // arrange: async companion test builds the isolated skill registry fixture.
    // act
    skill_load_hides_denied_or_invalid_skills();
    // assert: companion test verifies denied and malformed skills stay hidden.
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn shipped_starter_skill_pack_is_discoverable_from_repo_checkout() {
    let _guard = skills_registry_test_lock();
    let repo = repo_root();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-shipped-rust-best-practices"),
            json!({"name": "rust-best-practices"}),
        )
        .await
        .unwrap_or_abort();
    assert!(skill.display_text.contains("# Skill: rust-best-practices"));
    assert!(skill.display_text.contains("cargo fmt --all -- --check"));
    assert_eq!(
        skill
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(repo
            .join(".agent-harness/skills/rust-best-practices/SKILL.md")
            .display()
            .to_string()))
    );
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn harness_skill_pack_is_discoverable_from_repo_checkout() {
    let _guard = skills_registry_test_lock();
    let repo = repo_root();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-shipped-analyze"),
            json!({"name": "rust-best-practices"}),
        )
        .await
        .unwrap_or_abort();
    assert!(skill.display_text.contains("# Rust best practices"));
    assert!(skill.display_text.contains("harness workspace"));
    assert_eq!(
        skill
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(repo
            .join(".agent-harness/skills/rust-best-practices/SKILL.md")
            .display()
            .to_string()))
    );
}
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_load_reports_agent_hint_for_build() {
    let _guard = skills_registry_test_lock();
    let repo = repo_root();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();

    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-build-agent-hint"),
            json!({"name": "build"}),
        )
        .await
        .expect_err("build is an agent, not a skill");

    let message = err.to_string();
    assert!(message.contains("Skill \"build\" not found"));
    assert!(message.contains("`build` is an agent, not a skill"));
    assert!(message.contains("task"));
}
