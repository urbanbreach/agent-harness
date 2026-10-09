use harness_core::config::SkillsConfig;
use harness_tools::discover_skill_catalog_with_config;
use std::fs;

#[tokio::test]
async fn skill_discovery_is_read_only_and_reports_precedence_without_loading_bodies(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let project = root.path().join("project");
    let global = root.path().join("global");
    for path in [
        project.join(".harness/skills/review"),
        global.join("review"),
        project.join(".harness/skills/disabled"),
    ] {
        fs::create_dir_all(&path)?;
        let name = path.file_name().ok_or("skill name")?.to_string_lossy();
        fs::write(path.join("SKILL.md"), format!("---\nname: {name}\ndescription: >-\n  Review the code\n  and its tests.\nallowed_tools: [read, grep]\n---\nPRIVATE BODY SENTINEL\n"))?;
    }
    fs::create_dir_all(project.join(".git"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&global, project.join(".harness/skills/escape"))?;
    let config = SkillsConfig {
        project_roots: vec![".harness/skills".into()],
        global_roots: vec![global],
        disabled: vec!["disabled".into()],
        ..Default::default()
    };
    let catalog = discover_skill_catalog_with_config(&project, &config)?;
    assert_eq!(catalog.entries.len(), 3);
    let project_review = catalog
        .entries
        .iter()
        .find(|entry| entry.stable_id == "skill:project:review")
        .ok_or("project skill missing")?;
    assert!(project_review.loadable);
    assert!(!project_review.body_loaded);
    assert_eq!(project_review.description, "Review the code and its tests.");
    assert_eq!(project_review.allowed_tools, ["read", "grep"]);
    assert!(catalog
        .entries
        .iter()
        .any(|entry| entry.source_scope == "global" && entry.status.as_str() == "shadowed"));
    assert!(catalog.entries.iter().any(|entry| entry.name == "disabled"
        && !entry.loadable
        && entry.status.as_str() == "disabled"));
    assert!(!format!("{catalog:?}").contains("PRIVATE BODY SENTINEL"));
    assert!(!project.join(".harness/sessions").exists());
    let mut runtime = harness_core::coord::CoordinatorConfig::new(root.path().join("sessions"));
    runtime.tool_registry = std::sync::Arc::new(harness_tools::coordinator_registry_with_skills(
        Default::default(),
        config,
    ));
    runtime.permission_policy = harness_core::perm::PermissionPolicy::allow_all();
    let handle = harness_core::coord::spawn_coordinator(
        runtime,
        std::sync::Arc::new(harness_core::clock::FakeClock::new()),
        std::sync::Arc::new(harness_core::redact::DefaultRedactor::default()),
    );
    handle.start_run("skills", &project).await?;
    let actor = || harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None);
    assert!(handle
        .execute_agent_tool_call(
            actor(),
            None,
            "skill",
            serde_json::json!({"name":"disabled"})
        )
        .await
        .is_err());
    let skill = project.join(".harness/skills/review/SKILL.md");
    fs::write(
        &skill,
        "---\nname: review\ndescription: Review\n---\nInspect $ARGUMENTS carefully.\n",
    )?;
    let output = handle
        .execute_agent_tool_call(
            actor(),
            None,
            "skill",
            serde_json::json!({"name":"review","arguments":"ownership"}),
        )
        .await?;
    assert!(output.display_text.contains("Inspect ownership carefully."));
    fs::write(
        &skill,
        format!(
            "---\nname: review\ndescription: Review\n---\n{}",
            "$ARGUMENTS\n".repeat(100)
        ),
    )?;
    assert!(
        handle
            .execute_agent_tool_call(
                actor(),
                None,
                "skill",
                serde_json::json!({"name":"review","arguments":"x".repeat(16384)})
            )
            .await
            .is_err(),
        "substitution must stay below the skill size bound"
    );
    handle.stop_run().await?;
    Ok(())
}

#[test]
fn compatibility_skills_are_discovered_after_harness_roots(
) -> Result<(), Box<dyn std::error::Error>> {
    for (locations, winner, reverse) in [
        (
            vec!["project/.agents/skills"],
            "project/.agents/skills",
            false,
        ),
        (
            vec!["project/.agents/skills", "project/.harness/skills"],
            "project/.harness/skills",
            false,
        ),
        (
            vec!["project/.agents/skills", "project/.harness/skills"],
            "project/.harness/skills",
            true,
        ),
        (
            vec!["project/sub/.agents/skills", "project/.harness/skills"],
            "project/.harness/skills",
            false,
        ),
        (
            vec![
                "project/.agents/skills",
                "global/.harness/skills",
                "global/.agents/skills",
            ],
            "global/.harness/skills",
            true,
        ),
    ] {
        let root = tempfile::tempdir()?;
        let project = root.path().join("project");
        fs::create_dir_all(project.join(".git"))?;
        fs::create_dir(project.join("sub"))?;
        for location in &locations {
            let path = root.path().join(location).join("review");
            fs::create_dir_all(&path)?;
            fs::write(
                path.join("SKILL.md"),
                "---\nname: review\ndescription: Review\n---\nReview code",
            )?;
        }
        let mut config = SkillsConfig {
            global_roots: vec![
                root.path().join("global/.harness/skills"),
                root.path().join("global/.agents/skills"),
            ],
            ..Default::default()
        };
        if reverse {
            config.project_roots.reverse();
            config.global_roots.reverse();
        }
        let catalog = discover_skill_catalog_with_config(&project.join("sub"), &config)?;
        assert_eq!(catalog.entries.len(), locations.len());
        for entry in &catalog.entries {
            let wins = entry.root_path == root.path().join(winner);
            assert_eq!(entry.loadable, wins, "{}", entry.location.display());
            assert_eq!(
                entry.status,
                if wins {
                    harness_tools::SkillCatalogStatus::Loadable
                } else {
                    harness_tools::SkillCatalogStatus::Shadowed
                }
            );
        }
    }
    Ok(())
}
