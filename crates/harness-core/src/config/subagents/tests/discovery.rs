use super::*;

#[test]
fn trusted_preset_and_agent_discovery_keeps_source_precedence(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    let user = temp.path().join("user");
    let bundled = temp.path().join("bundled");
    for root in [&project.join(".grok"), &user, &bundled] {
        for folder in ["agents", "roles", "personas"] {
            std::fs::create_dir_all(root.join(folder))?;
        }
        std::fs::write(
            root.join("roles/research.toml"),
            format!("model = '{}'\n", root.display()),
        )?;
        std::fs::write(
            root.join("personas/reviewer.toml"),
            format!("instructions = '{}'\n", root.display()),
        )?;
    }
    std::fs::write(
        user.join("agents/explore.md"),
        "---\nname: explore\ndescription: User\n---\nUser body",
    )?;
    std::fs::write(
        project.join(".grok/roles/ignored.json"),
        "{\"model\":\"ignored\"}",
    )?;
    std::fs::write(project.join(".grok/personas/broken.toml"), "invalid = [")?;
    let mut settings = SubagentRuntimeConfig::default();
    let mut discovery = SubagentDiscoveryContext {
        cwd: project.clone(),
        project_trusted: true,
        home: None,
        user_root: Some(user.clone()),
        bundled_root: Some(bundled),
        plugins: Vec::new(),
        cli_definitions: Vec::new(),
    };
    let snapshot = discover_subagent_definitions(&settings, &discovery);
    assert_eq!(
        snapshot
            .definition("explore")
            .ok_or("builtin missing")?
            .source,
        SubagentDefinitionSource::Builtin
    );
    assert_eq!(
        snapshot.roles["research"].source_dir.as_deref(),
        Some(project.join(".grok/roles").as_path())
    );
    assert!(!snapshot.roles.contains_key("ignored"));
    assert!(!snapshot.warnings.is_empty());
    std::fs::write(project.join(".grok/agents/explore.md"), "---\nname: explore\ndescription: Project\nmaxTurns: 5\nmcpInheritance:\n  named: [A]\n---\nProject body")?;
    settings.roles.insert(
        "research".into(),
        SubagentRole {
            model: Some("inline".into()),
            ..SubagentRole::default()
        },
    );
    let snapshot = discover_subagent_definitions(&settings, &discovery);
    assert_eq!(snapshot.roles["research"].model.as_deref(), Some("inline"));
    assert_eq!(
        snapshot
            .definition("explore")
            .ok_or("project missing")?
            .source,
        SubagentDefinitionSource::Project
    );
    assert_eq!(
        snapshot
            .definition("explore")
            .ok_or("project missing")?
            .max_turns,
        NonZeroU32::new(5)
    );
    settings.roles.clear();
    discovery.project_trusted = false;
    let snapshot = discover_subagent_definitions(&settings, &discovery);
    assert_eq!(
        snapshot.roles["research"].source_dir.as_deref(),
        Some(user.join("roles").as_path())
    );
    assert_eq!(
        snapshot.personas["reviewer"].source_dir.as_deref(),
        Some(user.join("personas").as_path())
    );
    for name in ["first", "second"] {
        let directory = temp.path().join(name).join("agents");
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join("worker.md"), "---\nname: worker\ndescription: Plugin\npermissionMode: bypassPermissions\n---\nPlugin body")?;
        discovery.plugins.push(SubagentPluginDefinitions {
            name: name.into(),
            enabled: true,
            trusted: name == "first",
            agent_dirs: vec![directory],
        });
    }
    let snapshot = discover_subagent_definitions(&settings, &discovery);
    assert!(snapshot.definition("worker").is_none());
    let plugin = snapshot
        .definition("first:worker")
        .ok_or("qualified plugin missing")?;
    assert_eq!(plugin.permission_mode, SubagentPermissionMode::Default);
    assert!(snapshot
        .definition("second:worker")
        .ok_or("untrusted plugin missing")?
        .prompt_body
        .is_none());
    discovery.plugins[1].enabled = false;
    let snapshot = discover_subagent_definitions(&settings, &discovery);
    assert_eq!(
        snapshot
            .definition("worker")
            .ok_or("bare plugin missing")?
            .source,
        plugin.source
    );
    assert!(snapshot.definition("second:worker").is_none());
    discovery.cli_definitions.push(SubagentDefinition {
        name: "cli-only".into(),
        ..SubagentDefinition::default()
    });
    assert_eq!(
        discover_subagent_definitions(&settings, &discovery)
            .definition("cli-only")
            .ok_or("CLI fallback missing")?
            .source,
        SubagentDefinitionSource::Cli
    );
    Ok(())
}
