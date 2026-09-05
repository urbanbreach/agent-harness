use harness_tools::UnwrapOrAbort;
use harness_tools::{discover_skill_catalog, SkillCatalogStatus};

#[test]
fn v1_skill_catalog_reports_compact_metadata_without_body_leak() {
    // arrange
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());

    write_v1_skill(
        &repo.join(".agent-harness/skills"),
        "v1-contract",
        r#"name: v1-contract
description: V1 catalog metadata description
argument_hint: ISSUE-123
allowed_tools: read, grep
mcp: deferred-local-metadata
resources: bundled-reference-not-loaded"#,
        "BODY LEAK SENTINEL: only activation may expose this body.",
    );

    let catalog = discover_skill_catalog(&repo).unwrap_or_abort();
    let entry = catalog
        .active_entry("v1-contract")
        // act
        .unwrap_or_abort();

    // assert
    assert_eq!(entry.name, "v1-contract");
    assert_eq!(entry.stable_id, "skill:project:v1-contract");
    assert_eq!(entry.status, SkillCatalogStatus::Loadable);
    assert!(entry.loadable);
    assert_eq!(entry.permission_mode, "allow");
    assert_eq!(entry.source_scope, "project");
    assert_eq!(entry.argument_hint.as_deref(), Some("ISSUE-123"));
    assert_eq!(entry.allowed_tools, vec!["read", "grep"]);
    assert_eq!(entry.deferred_mcp.as_deref(), Some("deferred-local-metadata"));
    assert_eq!(
        entry.deferred_resources.as_deref(),
        Some("bundled-reference-not-loaded")
    );
    assert!(!entry.body_loaded);
    assert!(entry.body_digest.is_none());
    let serialized = serde_json::to_string(&catalog).unwrap_or_abort();
    assert!(!serialized.contains("BODY LEAK SENTINEL"));
    assert!(!serialized.contains("target_agent"));
    assert!(!serialized.contains("target_category"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn v1_skill_activation_reports_metadata_then_loads_body() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());

    write_v1_skill(
        &repo.join(".agent-harness/skills"),
        "activation-skill",
        "name: activation-skill\ndescription: Activation description\nallowed_tools: read",
        "ACTIVATION BODY SENTINEL.",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-v1-activation"),
            json!({"name": "activation-skill"}),
        )
        .await
        .unwrap_or_abort();

    assert!(skill.display_text.contains("ACTIVATION BODY SENTINEL"));
    let metadata = skill
        .structured_json
        .as_ref()
        .and_then(|value| value.get("metadata"))
        .unwrap_or_abort();
    assert_eq!(metadata["stable_id"], json!("skill:project:activation-skill"));
    assert_eq!(metadata["status"], json!("loadable"));
    assert_eq!(metadata["source_scope"], json!("project"));
    assert_eq!(metadata["allowed_tools"], json!(["read"]));
    assert_eq!(metadata["body_loaded"], json!(false));
    assert!(!metadata
        .to_string()
        .contains("ACTIVATION BODY SENTINEL"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn v1_skill_activation_loads_bundled_resources_with_caps_and_redaction() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let skill_root = repo.join(".agent-harness/skills");

    write_v1_skill(
        &skill_root,
        "resource-skill",
        "name: resource-skill\ndescription: Resource description\nresources: references/guide.md, references/large.md",
        "ACTIVATION BODY.",
    );
    let skill_dir = skill_root.join("resource-skill");
    fs::create_dir_all(skill_dir.join("references")).unwrap_or_abort();
    fs::write(
        skill_dir.join("references/guide.md"),
        "RESOURCE GUIDE SENTINEL\nBearer super.secret.token\n",
    )
    .unwrap_or_abort();
    fs::write(
        skill_dir.join("references/large.md"),
        format!("{}TRUNCATED TAIL SENTINEL", "x".repeat(70 * 1024)),
    )
    .unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-v1-resource"),
            json!({"name": "resource-skill"}),
        )
        .await
        .unwrap_or_abort();

    assert!(skill.display_text.contains("## Bundled resources"));
    assert!(skill
        .display_text
        .contains("<skill_resource path=\"references/guide.md\""));
    assert!(skill.display_text.contains("RESOURCE GUIDE SENTINEL"));
    assert!(skill.display_text.contains("Bearer [REDACTED]"));
    assert!(!skill.display_text.contains("super.secret.token"));
    assert!(skill
        .display_text
        .contains("<skill_resource path=\"references/large.md\""));
    assert!(skill.display_text.contains("truncated=\"true\""));
    assert!(!skill.display_text.contains("TRUNCATED TAIL SENTINEL"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn v1_skill_resources_reject_escape_paths_for_project_and_global_roots() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let repo = temp_dir.path().join("repo");
    let home = temp_dir.path().join("home");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::write(repo.join("outside-project.txt"), "PROJECT SECRET").unwrap_or_abort();
    fs::write(home.join("outside-global.txt"), "GLOBAL SECRET").unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![home.join(".config/agent-harness/skills")],
        ..SkillsConfig::default()
    });

    let cases = vec![
        (
            "project-dotdot".to_string(),
            repo.join(".agent-harness/skills"),
            "../outside-project.txt".to_string(),
        ),
        (
            "global-dotdot".to_string(),
            home.join(".config/agent-harness/skills"),
            "../outside-global.txt".to_string(),
        ),
        (
            "project-absolute".to_string(),
            repo.join(".agent-harness/skills"),
            repo.join("outside-project.txt").display().to_string(),
        ),
    ];
    for (name, root, resource) in cases {
        write_v1_skill(
            &root,
            &name,
            &format!("name: {name}\ndescription: Escape description\nresources: {resource}"),
            "BODY",
        );
    }

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    for name in ["project-dotdot", "global-dotdot", "project-absolute"] {
        let err = skill_tool
            .call(
                tool_context(&repo, &format!("toolcall-v1-resource-{name}")),
                json!({"name": name}),
            )
            .await
            .expect_err("resource escape should reject activation");
        let message = err.to_string();
        assert!(
            message.contains("resource") && message.contains("invalid"),
            "escape message should explain invalid resource path: {message}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn v1_skill_resources_reject_symlink_escape() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    fs::write(repo.join("outside-symlink.txt"), "SYMLINK SECRET").unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let skill_root = repo.join(".agent-harness/skills");

    write_v1_skill(
        &skill_root,
        "symlink-resource",
        "name: symlink-resource\ndescription: Symlink description\nresources: refs/link.md",
        "BODY",
    );
    let skill_dir = skill_root.join("symlink-resource");
    fs::create_dir_all(skill_dir.join("refs")).unwrap_or_abort();
    std::os::unix::fs::symlink(
        repo.join("outside-symlink.txt"),
        skill_dir.join("refs/link.md"),
    )
    .unwrap_or_abort();

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    let err = skill_tool
        .call(
            tool_context(&repo, "toolcall-v1-symlink-resource"),
            json!({"name": "symlink-resource"}),
        )
        .await
        .expect_err("symlink escape should reject activation");
    assert!(
        err.to_string().contains("outside its skill root"),
        "symlink escape message should mention skill root: {err}"
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn v1_skill_catalog_reports_shadowed_disabled_denied_and_malformed_states() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![home.join(".config/agent-harness/skills")],
        disabled: vec!["skill:project:disabled-skill".to_string()],
        permissions: BTreeMap::from([
            ("*".to_string(), PermissionMode::Allow),
            ("denied-*".to_string(), PermissionMode::Deny),
        ]),
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".agent-harness/skills"),
        "shadowed-skill",
        "Project description",
        "Project body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "shadowed-skill",
        "Global description",
        "Global body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "disabled-skill",
        "Disabled description",
        "Disabled body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "denied-secret",
        "Denied description",
        "Denied body",
    );
    write_v1_skill(
        &repo.join(".agent-harness/skills"),
        "malformed-skill",
        "name: malformed-skill\ndescription: Malformed description\ntool_permissions: edit",
        "Malformed body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "valid-sibling",
        "Valid sibling description",
        "Valid sibling body",
    );

    let catalog = discover_skill_catalog(&repo).unwrap_or_abort();
    assert_eq!(
        catalog.active_entry("shadowed-skill").unwrap_or_abort().source_scope,
        "project"
    );
    assert!(catalog
        .entries_for("shadowed-skill")
        .iter()
        .any(|entry| entry.status == SkillCatalogStatus::Shadowed
            && entry.source_scope == "global"));
    let disabled_entry = catalog
        .active_entry("disabled-skill")
        .unwrap_or_abort();
    assert_eq!(disabled_entry.status, SkillCatalogStatus::Disabled);
    assert_eq!(disabled_entry.stable_id, "skill:project:disabled-skill");
    assert!(!disabled_entry.loadable);
    assert_eq!(
        disabled_entry.reason.as_deref(),
        Some("disabled by skills.disabled")
    );
    assert_eq!(
        catalog
            .active_entry("denied-secret")
            .unwrap_or_abort()
            .status,
        SkillCatalogStatus::Denied
    );
    assert_eq!(
        catalog
            .active_entry("malformed-skill")
            .unwrap_or_abort()
            .status,
        SkillCatalogStatus::Malformed
    );
    assert_eq!(
        catalog
            .active_entry("valid-sibling")
            .unwrap_or_abort()
            .status,
        SkillCatalogStatus::Loadable
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();
    for name in ["disabled-skill", "denied-secret", "malformed-skill"] {
        let err = skill_tool
            .call(
                tool_context(&repo, &format!("toolcall-{name}")),
                json!({"name": name}),
            )
            .await
            .expect_err("unloadable V1 skill should not activate");
        let message = err.to_string();
        assert!(message.contains(name), "message should name {name}: {message}");
        assert!(
            message.contains("disabled")
                || message.contains("denied")
                || message.contains("malformed"),
            "message should explain V1 status for {name}: {message}"
        );
    }
}
