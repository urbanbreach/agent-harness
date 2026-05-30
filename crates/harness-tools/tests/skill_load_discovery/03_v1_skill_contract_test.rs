use harness_core::agent_catalog::{SHIPPED_CATEGORY_ROUTES, SHIPPED_SUBAGENTS};
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

fn section_body<'a>(body: &'a str, section: &str) -> &'a str {
    let section_start = body
        .find(section)
        .unwrap_or_else(|| panic!("missing section `{section}`"));
    let after_heading = &body[section_start + section.len()..];
    after_heading
        .split("\n## ")
        .next()
        .expect("section split always yields current section")
}

fn assert_section_has_content(body: &str, section: &str, skill_name: &str) {
    let content = section_body(body, section)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !content.is_empty(),
        "{skill_name} section `{section}` must contain non-empty guidance"
    );
}

fn quoted_values_after(body: &str, token: &str) -> Vec<String> {
    body.match_indices(token)
        .filter_map(|(index, _)| {
            let after_token = &body[index + token.len()..];
            let quote = after_token.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let after_quote = &after_token[quote.len_utf8()..];
            let end = after_quote.find(quote)?;
            Some(after_quote[..end].to_string())
        })
        .collect()
}

#[test]
fn v1_skill_catalog_reports_compact_metadata_without_body_leak() {
    // arrange
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
        // act
        .expect("active V1 skill metadata");

    // assert
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
    let skill_tool = registry.get("skill").expect("skill tool");

    let _loadable_guard = SkillsConfigGuard::install(skills_config_without_global_roots());

    for (name, body_marker) in [
        ("git-master", "Use git with atomic, reviewable intent"),
        ("review-work", "Run a structured post-implementation review"),
        ("frontend-ui-ux", "Improve visible UI surfaces"),
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
        ],
        ..SkillsConfig::default()
    });
    let catalog = discover_skill_catalog(&root).expect("discover disabled shipped skills");

    for name in ["git-master", "review-work", "frontend-ui-ux"] {
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
fn shipped_v1_builtin_skills_have_quality_contract_and_catalog_metadata() {
    // arrange
    let _guard = skills_registry_test_lock();
    let _skills_guard = SkillsConfigGuard::install(skills_config_without_global_roots());
    let root = repo_root();
    let skill_root = root.join(".agent-harness/skills");
    let catalog = discover_skill_catalog(&root).expect("discover shipped skill catalog");

    for name in ["git-master", "review-work", "frontend-ui-ux"] {
        let skill_path = skill_root.join(name).join("SKILL.md");
        let body = fs::read_to_string(&skill_path)
            .unwrap_or_else(|err| panic!("read shipped skill {}: {err}", skill_path.display()));
        for frontmatter in [
            "name:",
            "description:",
            "argument_hint:",
            "allowed_tools:",
            "target_agent:",
            "target_category:",
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
        .expect("read review-work skill");
    for real_route in [
        "category=\"deep\"",
        "category=\"ultrabrain\"",
        "category=\"unspecified-high\"",
        "run_in_background=true",
        "background_output",
    ] {
        assert!(review.contains(real_route), "review-work missing `{real_route}`");
    }
    assert!(!review.contains("oracle"), "review-work must not reference non-shipped oracle agent");
}

#[test]
fn review_work_references_only_shipped_agent_catalog_routes() {
    // arrange
    let review = fs::read_to_string(repo_root().join(".agent-harness/skills/review-work/SKILL.md"))
        .expect("read review-work skill");

    // act
    let category_refs = quoted_values_after(&review, "category=");
    let subagent_refs = quoted_values_after(&review, "subagent_type=");

    // assert
    assert!(
        !category_refs.is_empty(),
        "review-work should name at least one shipped category route"
    );
    for category in category_refs {
        assert!(
            SHIPPED_CATEGORY_ROUTES.contains(&category.as_str()),
            "review-work references non-shipped category `{category}`"
        );
    }
    for subagent in subagent_refs {
        assert!(
            SHIPPED_SUBAGENTS.contains(&subagent.as_str()),
            "review-work references non-shipped subagent `{subagent}`"
        );
    }
}
