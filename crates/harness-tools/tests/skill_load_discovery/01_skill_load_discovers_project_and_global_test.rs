use harness_tools::UnwrapOrAbort;
#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn skill_load_discovers_project_and_global_roots_with_precedence() {
    // arrange
    // act
    // assert
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    let app = repo.join("packages/app");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&app).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(skills_config_with_global_root(
        home.join(".config/agent-harness/skills"),
    ));

    write_skill(
        &home.join(".config/agent-harness/skills"),
        "shared-skill",
        "Global shared description",
        "Global body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "shared-skill",
        "Repo shared description",
        "Repo body",
    );
    write_skill(
        &app.join(".agent-harness/skills"),
        "shared-skill",
        "App shared description",
        "App body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "repo-only",
        "Repo only description",
        "Repo only body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "global-only",
        "Global only description",
        "Global only body",
    );

    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();

    let native_shared = skill_tool
        .call(
            tool_context(&app, "toolcall-native-shared"),
            json!({"name": "shared-skill"}),
        )
        .await
        .unwrap_or_abort();
    assert!(native_shared
        .display_text
        .contains("App shared description"));
    assert!(native_shared.display_text.contains("App body"));
    assert!(!native_shared.display_text.contains("Repo body"));
    assert!(!native_shared.display_text.contains("Global body"));
    assert_eq!(
        native_shared
            .structured_json
            .as_ref()
            .and_then(|value| value.get("location")),
        Some(&json!(app
            .join(".agent-harness/skills/shared-skill/SKILL.md")
            .display()
            .to_string()))
    );

    let repo_only = skill_tool
        .call(
            tool_context(&app, "toolcall-repo-only"),
            json!({"name": "repo-only"}),
        )
        .await
        .unwrap_or_abort();
    assert!(repo_only.display_text.contains("Repo only description"));

    let global_only = skill_tool
        .call(
            tool_context(&app, "toolcall-global-only"),
            json!({"name": "global-only"}),
        )
        .await
        .unwrap_or_abort();
    assert!(global_only.display_text.contains("Global only description"));
}
#[test]
fn skill_discovery_walks_project_and_global_roots() {
    // arrange
    // act
    // assert
    skill_load_discovers_project_and_global_roots_with_precedence();
}

#[test]
fn skill_discovery_ignores_external_adapter_roots_unless_explicitly_configured() {
    // arrange
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![home.join(".config/agent-harness/skills")],
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".agent-harness/skills"),
        "harness-owned",
        "Harness-owned project description",
        "Harness-owned project body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "harness-global",
        "Harness-owned global description",
        "Harness-owned global body",
    );
    for root in [
        ".external-editor/skills",
        ".assistant/skills",
        ".agents/skills",
    ] {
        write_skill(
            &repo.join(root),
            "external-project-adapter",
            "External project description",
            "External project body",
        );
    }
    for root in [
        home.join(".external-editor/skills"),
        home.join(".assistant/skills"),
        home.join(".agents/skills"),
    ] {
        write_skill(
            &root,
            "external-global-adapter",
            "External global description",
            "External global body",
        );
    }

    // act
    let default_catalog =
        harness_tools::discover_skill_catalog(&repo).unwrap_or_abort();

    // assert
    assert!(
        default_catalog.active_entry("harness-owned").is_some(),
        "Harness-owned project root should be active by default"
    );
    assert!(
        default_catalog.active_entry("harness-global").is_some(),
        "configured Harness-owned global root should be active"
    );
    assert!(
        default_catalog
            .active_entry("external-project-adapter")
            .is_none(),
        "external project adapter roots must not be searched by default"
    );
    assert!(
        default_catalog
            .active_entry("external-global-adapter")
            .is_none(),
        "external global adapter roots must not be searched by default"
    );

    drop(_skills_guard);
    let _explicit_roots_guard = SkillsConfigGuard::install(SkillsConfig {
        project_roots: vec![
            PathBuf::from(".external-editor/skills"),
            PathBuf::from(".assistant/skills"),
            PathBuf::from(".agents/skills"),
        ],
        global_roots: vec![home.join(".agents/skills")],
        ..SkillsConfig::default()
    });
    let explicit_catalog =
        harness_tools::discover_skill_catalog(&repo).unwrap_or_abort();
    assert!(
        explicit_catalog
            .active_entry("external-project-adapter")
            .is_some(),
        "external project adapter roots should load only when explicitly configured"
    );
    assert!(
        explicit_catalog
            .active_entry("external-global-adapter")
            .is_some(),
        "external global adapter roots should load only when explicitly configured"
    );
}

#[test]
fn default_harness_owned_roots_resolve_duplicates_before_global_roots() {
    // arrange
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![home.join(".config/agent-harness/skills")],
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".harness/skills"),
        "default-conflict",
        "Harness fallback description",
        "Harness fallback body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "default-conflict",
        "Harness primary description",
        "Harness primary body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "default-conflict",
        "Harness global description",
        "Harness global body",
    );

    // act
    let catalog = harness_tools::discover_skill_catalog(&repo).unwrap_or_abort();
    let entries = catalog.entries_for("default-conflict");

    // assert
    assert_eq!(entries.len(), 3, "duplicate diagnostics stay catalog-visible");
    assert_eq!(entries[0].description, "Harness primary description");
    assert_eq!(entries[0].status, harness_tools::SkillCatalogStatus::Loadable);
    assert_eq!(entries[0].stable_id, "skill:project:default-conflict");
    assert_eq!(
        entries[0].location,
        repo.join(".agent-harness/skills/default-conflict/SKILL.md")
            .canonicalize()
            .unwrap_or_abort()
    );
    assert_eq!(entries[1].status, harness_tools::SkillCatalogStatus::Shadowed);
    assert_eq!(
        entries[1].location,
        repo.join(".harness/skills/default-conflict/SKILL.md")
            .canonicalize()
            .unwrap_or_abort()
    );
    assert_eq!(entries[2].status, harness_tools::SkillCatalogStatus::Shadowed);
    assert_eq!(entries[2].stable_id, "skill:global:default-conflict");
}

#[test]
fn imported_compatibility_roots_do_not_shadow_harness_owned_project_skills() {
    // arrange
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();

    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        project_roots: vec![
            PathBuf::from(".external-editor/skills"),
            PathBuf::from(".assistant/skills"),
            PathBuf::from(".agents/skills"),
            PathBuf::from(".agent-harness/skills"),
            PathBuf::from(".harness/skills"),
        ],
        global_roots: vec![
            home.join(".agents/skills"),
            home.join(".config/agent-harness/skills"),
        ],
        ..SkillsConfig::default()
    });

    write_skill(
        &repo.join(".external-editor/skills"),
        "conflict-skill",
        "External editor description",
        "External editor body",
    );
    write_skill(
        &repo.join(".assistant/skills"),
        "conflict-skill",
        "Assistant description",
        "Assistant body",
    );
    write_skill(
        &repo.join(".agents/skills"),
        "conflict-skill",
        "Agent adapter description",
        "Agent adapter body",
    );
    write_skill(
        &repo.join(".harness/skills"),
        "conflict-skill",
        "Harness fallback description",
        "Harness fallback body",
    );
    write_skill(
        &repo.join(".agent-harness/skills"),
        "conflict-skill",
        "Harness-owned project description",
        "Harness-owned project body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "conflict-skill",
        "Harness global description",
        "Harness global body",
    );
    write_skill(
        &home.join(".agents/skills"),
        "conflict-skill",
        "User agent adapter description",
        "User agent adapter body",
    );

    // act
    let catalog = harness_tools::discover_skill_catalog(&repo).unwrap_or_abort();
    let active = catalog
        .active_entry("conflict-skill")
        .unwrap_or_abort();

    // assert
    assert_eq!(active.status, harness_tools::SkillCatalogStatus::Loadable);
    assert_eq!(active.stable_id, "skill:project:conflict-skill");
    assert_eq!(active.description, "Harness-owned project description");
    assert_eq!(
        active.location,
        repo.join(".agent-harness/skills/conflict-skill/SKILL.md")
            .canonicalize()
            .unwrap_or_abort()
    );
    let shadowed = catalog
        .entries_for("conflict-skill")
        .into_iter()
        .filter(|entry| entry.status == harness_tools::SkillCatalogStatus::Shadowed)
        .collect::<Vec<_>>();
    assert_eq!(shadowed.len(), 6, "all lower-precedence duplicates are diagnostics-visible");
    assert!(shadowed.iter().all(|entry| entry.reason.as_deref()
        == Some("shadowed by a higher-precedence `conflict-skill` skill")));
}

#[test]
fn imported_compatibility_roots_are_ordered_after_non_compat_roots() {
    // arrange
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    let repo = temp_dir.path().join("repo");
    fs::create_dir_all(&home).unwrap_or_abort();
    fs::create_dir_all(&repo).unwrap_or_abort();
    fs::create_dir_all(repo.join(".git")).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        project_roots: vec![
            PathBuf::from(".agents/skills"),
            PathBuf::from(".assistant/skills"),
            PathBuf::from(".external-editor/skills"),
        ],
        global_roots: vec![
            home.join(".agents/skills"),
            home.join(".config/agent-harness/skills"),
        ],
        ..SkillsConfig::default()
    });

    for root in [
        repo.join(".agents/skills"),
        repo.join(".assistant/skills"),
        repo.join(".external-editor/skills"),
        home.join(".agents/skills"),
    ] {
        write_skill(
            &root,
            "compat-only",
            &format!("{} description", root.display()),
            &format!("{} body", root.display()),
        );
    }
    write_skill(
        &repo.join(".agents/skills"),
        "global-harness-conflict",
        "Project compatibility description",
        "Project compatibility body",
    );
    write_skill(
        &home.join(".config/agent-harness/skills"),
        "global-harness-conflict",
        "Harness global description",
        "Harness global body",
    );

    // act
    let catalog = harness_tools::discover_skill_catalog(&repo).unwrap_or_abort();
    let compat_entries = catalog.entries_for("compat-only");
    let harness_conflict = catalog
        .active_entry("global-harness-conflict")
        .unwrap_or_abort();

    // assert
    assert_eq!(compat_entries.len(), 4);
    assert_eq!(compat_entries[0].status, harness_tools::SkillCatalogStatus::Loadable);
    assert_eq!(
        compat_entries[0].location,
        repo.join(".agents/skills/compat-only/SKILL.md")
            .canonicalize()
            .unwrap_or_abort()
    );
    assert_eq!(
        compat_entries[1].location,
        repo.join(".assistant/skills/compat-only/SKILL.md")
            .canonicalize()
            .unwrap_or_abort()
    );
    assert_eq!(
        compat_entries[2].location,
        repo.join(".external-editor/skills/compat-only/SKILL.md")
            .canonicalize()
            .unwrap_or_abort()
    );
    assert_eq!(compat_entries[3].source_scope, "global");
    assert!(compat_entries[1..]
        .iter()
        .all(|entry| entry.status == harness_tools::SkillCatalogStatus::Shadowed));

    assert_eq!(harness_conflict.description, "Harness global description");
    assert_eq!(harness_conflict.source_scope, "global");
    assert_eq!(harness_conflict.stable_id, "skill:global:global-harness-conflict");
}
