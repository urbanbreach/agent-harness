use harness_tools::UnwrapOrAbort;
use harness_tools::{discover_skill_catalog, SkillCatalogStatus};

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "the global registry lock intentionally serializes skills registry mutation across awaits"
)]
async fn shipped_v1_builtin_skills_load_and_disable_by_stable_id() {
    // arrange
    let _guard = skills_registry_test_lock();
    let root = repo_root();
    let registry = coordinator_registry(ShellAllowlist::default());
    let skill_tool = registry.get("skill").unwrap_or_abort();

    let _loadable_guard = SkillsConfigGuard::install(skills_config_without_global_roots());

    for (name, body_marker) in [
        ("git-master", "Use git with atomic, reviewable intent"),
        ("review-work", "Run a structured post-implementation review"),
        ("frontend-ui-ux", "Improve visible UI surfaces"),
        ("harness-qa", "Run offline agent dogfood"),
    ] {
        // act
        let loaded = skill_tool
            .call(
                tool_context(&root, &format!("toolcall-shipped-{name}")),
                json!({"name": name}),
            )
            .await
            .unwrap_or_else(|err| panic!("load shipped skill {name}: {err}"));

        // assert
        assert!(
            loaded.display_text.contains(body_marker),
            "{name} body marker should be visible only on activation"
        );
        let metadata = loaded
            .structured_json
            .as_ref()
            .and_then(|value| value.get("metadata"))
            .unwrap_or_else(|| panic!("metadata for {name}"));
        assert_eq!(metadata["stable_id"], json!(format!("skill:project:{name}")));
        assert_eq!(metadata["status"], json!("loadable"));
        assert_eq!(metadata["source_scope"], json!("project"));
        assert_eq!(metadata["body_loaded"], json!(false));
    }
    drop(_loadable_guard);

    let _disabled_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: Vec::new(),
        disabled: vec![
            "skill:project:git-master".to_string(),
            "skill:project:review-work".to_string(),
            "skill:project:frontend-ui-ux".to_string(),
            "skill:project:harness-qa".to_string(),
        ],
        ..SkillsConfig::default()
    });
    let catalog = discover_skill_catalog(&root).unwrap_or_abort();

    for name in ["git-master", "review-work", "frontend-ui-ux", "harness-qa"] {
        let entry = catalog
            .active_entry(name)
            .unwrap_or_else(|| panic!("catalog missing disabled built-in {name}"));
        assert_eq!(entry.stable_id, format!("skill:project:{name}"));
        assert_eq!(entry.status, SkillCatalogStatus::Disabled);
        assert_eq!(entry.reason.as_deref(), Some("disabled by skills.disabled"));

        let err = skill_tool
            .call(
                tool_context(&root, &format!("toolcall-disabled-{name}")),
                json!({"name": name}),
            )
            .await
            .expect_err("disabled built-in should not load");
        let message = err.to_string();
        assert!(message.contains(name), "disabled error should name {name}: {message}");
        assert!(
            message.contains("disabled by skills.disabled"),
            "disabled error should explain stable-id disablement for {name}: {message}"
        );
    }
}

#[test]
fn shipped_builtin_skill_ids_win_over_global_and_imported_duplicates() {
    // arrange
    let _guard = skills_registry_test_lock();
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let home = temp_dir.path().join("home");
    fs::create_dir_all(&home).unwrap_or_abort();
    let _skills_guard = SkillsConfigGuard::install(SkillsConfig {
        global_roots: vec![
            home.join(".agents/skills"),
            home.join(".config/agent-harness/skills"),
        ],
        ..SkillsConfig::default()
    });
    let root = repo_root();

    for name in ["git-master", "review-work", "frontend-ui-ux", "harness-qa"] {
        write_skill(
            &home.join(".agents/skills"),
            name,
            "Imported duplicate description",
            "Imported duplicate body",
        );
        write_skill(
            &home.join(".config/agent-harness/skills"),
            name,
            "Global duplicate description",
            "Global duplicate body",
        );
    }

    // act
    let catalog = discover_skill_catalog(&root).unwrap_or_abort();

    // assert
    for name in ["git-master", "review-work", "frontend-ui-ux", "harness-qa"] {
        let active = catalog
            .active_entry(name)
            .unwrap_or_else(|| panic!("catalog missing shipped skill {name}"));
        assert_eq!(active.stable_id, format!("skill:project:{name}"));
        assert_eq!(active.status, SkillCatalogStatus::Loadable);
        assert_eq!(active.source_scope, "project");
        assert_eq!(
            active.location,
            root.join(".agent-harness/skills").join(name).join("SKILL.md")
        );

        let shadowed = catalog
            .entries_for(name)
            .into_iter()
            .filter(|entry| entry.status == SkillCatalogStatus::Shadowed)
            .collect::<Vec<_>>();
        assert_eq!(shadowed.len(), 2, "global and imported duplicates are shadowed");
        assert!(shadowed
            .iter()
            .any(|entry| entry.stable_id == format!("skill:global:{name}")));
    }
}

#[test]
fn shipped_v1_builtin_skills_have_quality_contract_and_catalog_metadata() {
    // arrange
    let _guard = skills_registry_test_lock();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let root = repo_root();
    let skill_root = root.join(".agent-harness/skills");
    let catalog = discover_skill_catalog(&root).unwrap_or_abort();

    for name in ["git-master", "review-work", "frontend-ui-ux", "harness-qa"] {
        let skill_path = skill_root.join(name).join("SKILL.md");
        let body = fs::read_to_string(&skill_path)
            .unwrap_or_else(|err| panic!("read shipped skill {}: {err}", skill_path.display()));
        for frontmatter in [
            "name:",
            "description:",
            "argument_hint:",
            "allowed_tools:",
            "mcp:",
            "resources:",
        // act
        ] {
            // assert
            assert!(body.contains(frontmatter), "{name} missing frontmatter `{frontmatter}`");
        }
        for section in [
            "## Purpose",
            "## Use When",
            "## Do Not Use When",
            "## Execution Policy",
            "## Steps",
            "## Tool Usage",
            "## Escalation and Stop Conditions",
            "## Final Checklist",
            "## Advanced Notes",
        ] {
            assert!(body.contains(section), "{name} missing section `{section}`");
            assert_section_has_content(&body, section, name);
        }

        let entry = catalog
            .active_entry(name)
            .unwrap_or_else(|| panic!("catalog missing built-in skill {name}"));
        assert_eq!(entry.stable_id, format!("skill:project:{name}"));
        assert_eq!(entry.status, SkillCatalogStatus::Loadable);
        assert!(entry.loadable);
        assert_eq!(entry.source_scope, "project");
        assert!(!entry.description.trim().is_empty());
    }

    let review = fs::read_to_string(skill_root.join("review-work/SKILL.md"))
        .unwrap_or_abort();
    for real_route in [
        "run_in_background=true",
        "background_output",
    ] {
        assert!(review.contains(real_route), "review-work missing `{real_route}`");
    }
    assert!(!review.contains("oracle"), "review-work must not reference non-shipped oracle agent");
}
