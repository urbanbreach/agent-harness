use super::*;

#[test]
fn discovery_and_model_resolution_follow_harness_roots_and_roles(
) -> Result<(), Box<dyn std::error::Error>> {
    // Each row resolves a real discovered definition, including built-in fallback.
    for (
        project_model,
        user_model,
        role_model,
        pin,
        call_model,
        definition_variant,
        call_variant,
        expected_model,
        expected_variant,
        rejected,
    ) in [
        (
            None,
            Some("local/user"),
            None,
            None,
            None,
            None,
            None,
            "local:user",
            None,
            false,
        ),
        (
            Some("local/project"),
            Some("local/user"),
            None,
            None,
            None,
            None,
            None,
            "local:project",
            None,
            false,
        ),
        (
            None, None, None, None, None, None, None, "parent", None, false,
        ),
        (
            None,
            None,
            Some("local/smol"),
            None,
            None,
            None,
            None,
            "local:smol",
            None,
            false,
        ),
        (
            Some("@smol"),
            None,
            None,
            None,
            None,
            None,
            None,
            "parent",
            None,
            false,
        ),
        (
            Some("@smol"),
            None,
            Some("local/smol"),
            None,
            None,
            None,
            None,
            "local:smol",
            None,
            false,
        ),
        (
            None,
            None,
            Some("local/smol"),
            Some("@smol"),
            None,
            None,
            None,
            "local:smol",
            None,
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            Some("@smol"),
            None,
            None,
            None,
            "parent",
            None,
            false,
        ),
        (
            Some("local/project"),
            None,
            Some("local/smol"),
            None,
            Some("@smol"),
            None,
            None,
            "local:smol",
            None,
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            Some("@smol"),
            None,
            None,
            "parent",
            None,
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            None,
            Some("low"),
            None,
            "local:project",
            Some("low"),
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            None,
            Some("low"),
            Some("high"),
            "local:project",
            Some("high"),
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            Some("local/project/high"),
            Some("low"),
            None,
            "local:project",
            Some("high"),
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            Some("local/project/high"),
            None,
            Some("low"),
            "local:project",
            Some("low"),
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            None,
            Some("missing"),
            None,
            "local:project",
            None,
            false,
        ),
        (
            None,
            None,
            Some("local/smol/high"),
            None,
            None,
            None,
            None,
            "local:smol",
            Some("high"),
            false,
        ),
        (
            Some("local/project"),
            None,
            Some("local/smol"),
            None,
            Some("@slow"),
            None,
            None,
            "local:smol",
            None,
            false,
        ),
        (
            Some("local/project"),
            None,
            None,
            None,
            Some("@slow"),
            None,
            None,
            "parent",
            None,
            false,
        ),
        (
            None,
            None,
            None,
            None,
            Some("@x"),
            None,
            None,
            "",
            None,
            true,
        ),
    ] {
        let temp = tempfile::tempdir()?;
        let home = temp.path().join("home");
        let user = temp.path().join("harness-home");
        let project = temp.path().join("project");
        let cwd = project.join("nested");
        std::fs::create_dir_all(project.join(".git"))?;
        std::fs::create_dir_all(&cwd)?;
        if project_model.is_some() {
            std::fs::create_dir_all(project.join(".harness/agents"))?;
            std::fs::write(
                project.join(".harness/agents/scout.md"),
                "---\nname: scout\nmodel: local/user\n---\nAncestor scout",
            )?;
        }
        for root in [
            project.join(".claude"),
            home.join(".claude"),
            home.join(".harness"),
            home.join(".agent-harness"),
            user.join("bundled"),
        ] {
            std::fs::create_dir_all(root.join("agents"))?;
            std::fs::write(
                root.join("agents/scout.md"),
                "---\nname: scout\nmodel: local/ignored\n---\nIgnored",
            )?;
            std::fs::write(
                root.join("agents/legacy.md"),
                "---\nname: legacy\n---\nIgnored",
            )?;
        }
        for (root, model) in [
            (cwd.join(".harness"), project_model),
            (user.clone(), user_model),
        ] {
            if let Some(model) = model {
                std::fs::create_dir_all(root.join("agents"))?;
                let variant = definition_variant
                    .map_or(String::new(), |variant| format!("variant: {variant}\n"));
                std::fs::write(
                    root.join("agents/scout.md"),
                    format!("---\nname: scout\nmodel: '{model}'\n{variant}---\nCustom scout"),
                )?;
            }
        }
        let mut settings = SubagentRuntimeConfig::default();
        settings.model_roles.smol = role_model.map(str::to_owned);
        settings.model_roles.slow = role_model.map(str::to_owned);
        if let Some(pin) = pin {
            settings.models.insert("scout".into(), pin.into());
        }
        for model in ["local:project", "local:smol"] {
            settings
                .model_variants
                .insert(model.into(), vec!["low".into(), "high".into()]);
        }
        let user_root = crate::storage_paths::data_dir_from_lookup(&|key| match key {
            "HOME" => Some(home.to_string_lossy().into_owned()),
            "HARNESS_HOME" => Some(user.to_string_lossy().into_owned()),
            _ => None,
        });
        let definitions = discover_subagent_definitions(
            &settings,
            &SubagentDiscoveryContext {
                cwd,
                project_trusted: true,
                user_root,
            },
        );
        assert!(definitions.definition("legacy").is_none());
        let catalog = catalog(&[
            "parent",
            "local:user",
            "local:project",
            "local:smol",
            "local:ignored",
        ]);
        let request = SubagentDefinitionRequest {
            subagent_type: "scout".into(),
            type_specified: true,
            model: call_model.map(str::to_owned),
            variant: call_variant.map(str::to_owned),
            ..Default::default()
        };
        let mut context = parent_context(&definitions, &catalog, &[]);
        if call_model.is_some_and(|model| matches!(model, "@smol" | "@slow"))
            && role_model.is_none()
        {
            context.catalog = None;
        }
        let result = resolve_subagent_definition(&settings, &request, &context);
        if rejected {
            assert!(matches!(
                result,
                Err(SubagentResolutionError::InvalidModel { .. })
            ));
        } else {
            let resolved = result?;
            assert_eq!(
                (resolved.model.as_str(), resolved.variant.as_deref()),
                (expected_model, expected_variant)
            );
            if definition_variant == Some("missing") {
                assert_eq!(resolved.warnings.len(), 1);
            }
        }
    }
    assert!(parse_subagent_definition("---\nname: worker\nmodel: '@x'\n---\nWorker").is_err());
    for raw in [
        r#"{"subagents":{"models":{"worker":"@x"}}}"#,
        r#"{"small_model":"local/user"}"#,
        r#"{"smallModel":"local/user"}"#,
    ] {
        assert!(load_config_from_str(raw).is_err());
    }
    Ok(())
}

#[test]
fn trusted_preset_discovery_uses_nearest_project_then_user(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    let cwd = project.join("nested");
    let user = temp.path().join("user");
    std::fs::create_dir_all(project.join(".git"))?;
    for root in [cwd.join(".harness"), project.join(".harness"), user.clone()] {
        for folder in ["roles", "personas"] {
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
    let mut settings = SubagentRuntimeConfig::default();
    let mut discovery = SubagentDiscoveryContext {
        cwd: cwd.clone(),
        project_trusted: true,
        user_root: Some(user.clone()),
    };
    for (trusted, expected) in [(true, cwd.join(".harness")), (false, user)] {
        discovery.project_trusted = trusted;
        let snapshot = discover_subagent_definitions(&settings, &discovery);
        assert_eq!(
            snapshot.roles["research"].source_dir,
            Some(expected.join("roles"))
        );
        assert_eq!(
            snapshot.personas["reviewer"].source_dir,
            Some(expected.join("personas"))
        );
    }
    settings.roles.insert(
        "research".into(),
        SubagentRole {
            model: Some("inline".into()),
            ..Default::default()
        },
    );
    let snapshot = discover_subagent_definitions(&settings, &discovery);
    assert_eq!(snapshot.roles["research"].model.as_deref(), Some("inline"));
    Ok(())
}
