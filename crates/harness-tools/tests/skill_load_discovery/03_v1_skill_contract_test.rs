use harness_tools::{discover_skill_catalog, SkillCatalogStatus};

fn write_v1_skill(root: &Path, name: &str, frontmatter: &str, body: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create v1 skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\n{frontmatter}\n---\n\n{body}\n"),
    )
    .expect("write v1 skill file");
}

#[test]
fn v1_skill_catalog_reports_compact_metadata_without_body_leak() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());

    write_v1_skill(
        &repo.join(".agent-harness/skills"),
        "v1-contract",
        r#"name: v1-contract
description: V1 catalog metadata description
argument_hint: ISSUE-123
allowed_tools: read, grep
target_agent: build
target_category: deep
mcp: deferred-local-metadata
resources: bundled-reference-not-loaded"#,
        "BODY LEAK SENTINEL: only activation may expose this body.",
    );

    let catalog = discover_skill_catalog(&repo).expect("discover compact catalog");
    let entry = catalog
        .active_entry("v1-contract")
        .expect("active V1 skill metadata");

    assert_eq!(entry.name, "v1-contract");
    assert_eq!(entry.stable_id, "skill:project:v1-contract");
    assert_eq!(entry.status, SkillCatalogStatus::Loadable);
    assert!(entry.loadable);
    assert_eq!(entry.permission_mode, "allow");
    assert_eq!(entry.source_scope, "project");
    assert_eq!(entry.argument_hint.as_deref(), Some("ISSUE-123"));
    assert_eq!(entry.allowed_tools, vec!["read", "grep"]);
    assert_eq!(entry.target_agent.as_deref(), Some("build"));
    assert_eq!(entry.target_category.as_deref(), Some("deep"));
    assert_eq!(entry.deferred_mcp.as_deref(), Some("deferred-local-metadata"));
    assert_eq!(
        entry.deferred_resources.as_deref(),
        Some("bundled-reference-not-loaded")
    );
    assert!(!entry.body_loaded);
    assert!(entry.body_digest.is_none());
    let serialized = serde_json::to_string(&catalog).expect("serialize catalog");
    assert!(!serialized.contains("BODY LEAK SENTINEL"));
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn v1_skill_activation_reports_metadata_then_loads_body() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());

    write_v1_skill(
        &repo.join(".agent-harness/skills"),
        "activation-skill",
        "name: activation-skill\ndescription: Activation description\nallowed_tools: read",
        "ACTIVATION BODY SENTINEL.",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
    let skill = skill_tool
        .call(
            tool_context(&repo, "toolcall-v1-activation"),
            json!({"name": "activation-skill"}),
        )
        .await
        .expect("activation skill loads");

    assert!(skill.display_text.contains("ACTIVATION BODY SENTINEL"));
    let metadata = skill
        .structured_json
        .as_ref()
        .and_then(|value| value.get("metadata"))
        .expect("activation metadata");
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
async fn v1_skill_catalog_reports_shadowed_disabled_denied_and_malformed_states() {
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).expect("home dir");
    fs::create_dir_all(repo.join(".git")).expect("git dir");
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

    let catalog = discover_skill_catalog(&repo).expect("discover status catalog");
    assert_eq!(
        catalog.active_entry("shadowed-skill").expect("active shadowed skill").source_scope,
        "project"
    );
    assert!(catalog
        .entries_for("shadowed-skill")
        .iter()
        .any(|entry| entry.status == SkillCatalogStatus::Shadowed
            && entry.source_scope == "global"));
    let disabled_entry = catalog
        .active_entry("disabled-skill")
        .expect("disabled entry");
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
            .expect("denied entry")
            .status,
        SkillCatalogStatus::Denied
    );
    assert_eq!(
        catalog
            .active_entry("malformed-skill")
            .expect("malformed entry")
            .status,
        SkillCatalogStatus::Malformed
    );
    assert_eq!(
        catalog
            .active_entry("valid-sibling")
            .expect("valid sibling survives malformed skill")
            .status,
        SkillCatalogStatus::Loadable
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").expect("skill tool");
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
