use super::*;

#[test]
fn permission_mode_override_only_uses_operator_modes() -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog(&["parent"]);
    let mut definitions = SubagentDefinitionSnapshot::default();
    let auto_definition: SubagentDefinition =
        serde_json::from_value(serde_json::json!({"permissionMode": "auto"}))?;
    assert_eq!(
        serde_json::to_value(&auto_definition)?["permissionMode"],
        "auto"
    );
    let own_modes = [
        SubagentPermissionMode::Default,
        SubagentPermissionMode::Plan,
        SubagentPermissionMode::AcceptEdits,
        SubagentPermissionMode::BypassPermissions,
        SubagentPermissionMode::DontAsk,
        auto_definition.permission_mode,
    ];
    let parent_modes = [
        None,
        Some(SubagentPermissionMode::Default),
        Some(SubagentPermissionMode::Plan),
        Some(SubagentPermissionMode::DontAsk),
    ];
    let request = SubagentDefinitionRequest {
        subagent_type: "worker".into(),
        type_specified: true,
        ..SubagentDefinitionRequest::default()
    };
    for own_mode in own_modes {
        definitions.project.insert(
            "worker".into(),
            SubagentDefinition {
                name: "worker".into(),
                permission_mode: own_mode,
                source: SubagentDefinitionSource::Project,
                ..SubagentDefinition::default()
            },
        );
        for parent_mode in parent_modes {
            let mut context = parent_context(&definitions, &catalog, &[]);
            context.parent_permission_mode = parent_mode;
            assert_eq!(
                resolve_subagent_definition(
                    &SubagentRuntimeConfig::default(),
                    &request,
                    &context,
                )?
                .permission_mode,
                own_mode,
                "parent override {parent_mode:?} must retain definition mode {own_mode:?}"
            );
        }
        for parent_mode in [
            SubagentPermissionMode::AcceptEdits,
            SubagentPermissionMode::BypassPermissions,
            auto_definition.permission_mode,
        ] {
            let mut context = parent_context(&definitions, &catalog, &[]);
            context.parent_permission_mode = Some(parent_mode);
            assert_eq!(
                resolve_subagent_definition(
                    &SubagentRuntimeConfig::default(),
                    &request,
                    &context,
                )?
                .permission_mode,
                parent_mode,
                "operator override {parent_mode:?} must replace definition mode {own_mode:?}"
            );
        }
    }

    definitions.plugins.insert(
        "plugin:worker".into(),
        SubagentDefinition {
            name: "plugin:worker".into(),
            permission_mode: SubagentPermissionMode::BypassPermissions,
            source: SubagentDefinitionSource::Plugin {
                name: "plugin".into(),
            },
            ..SubagentDefinition::default()
        },
    );
    let plugin_request = SubagentDefinitionRequest {
        subagent_type: "plugin:worker".into(),
        type_specified: true,
        ..SubagentDefinitionRequest::default()
    };
    let mut plugin_context = parent_context(&definitions, &catalog, &[]);
    plugin_context.parent_permission_mode = Some(SubagentPermissionMode::BypassPermissions);
    assert_eq!(
        resolve_subagent_definition(
            &SubagentRuntimeConfig::default(),
            &plugin_request,
            &plugin_context,
        )?
        .permission_mode,
        SubagentPermissionMode::Default
    );
    plugin_context.parent_permission_mode = Some(auto_definition.permission_mode);
    assert_eq!(
        resolve_subagent_definition(
            &SubagentRuntimeConfig::default(),
            &plugin_request,
            &plugin_context,
        )?
        .permission_mode,
        SubagentPermissionMode::Default
    );

    definitions.project.insert(
        "worker".into(),
        SubagentDefinition {
            name: "worker".into(),
            permission_mode: SubagentPermissionMode::BypassPermissions,
            source: SubagentDefinitionSource::Project,
            ..SubagentDefinition::default()
        },
    );
    let mut managed_context = parent_context(&definitions, &catalog, &[]);
    managed_context.managed_block_bypass = true;
    assert_eq!(
        resolve_subagent_definition(
            &SubagentRuntimeConfig::default(),
            &request,
            &managed_context,
        )?
        .permission_mode,
        SubagentPermissionMode::Default
    );
    managed_context.parent_permission_mode = Some(SubagentPermissionMode::AcceptEdits);
    assert_eq!(
        resolve_subagent_definition(
            &SubagentRuntimeConfig::default(),
            &request,
            &managed_context,
        )?
        .permission_mode,
        SubagentPermissionMode::AcceptEdits
    );
    managed_context.parent_permission_mode = Some(auto_definition.permission_mode);
    assert_eq!(
        resolve_subagent_definition(
            &SubagentRuntimeConfig::default(),
            &request,
            &managed_context,
        )?
        .permission_mode,
        auto_definition.permission_mode
    );
    Ok(())
}

#[test]
fn definition_gates_replace_only_omitted_type_and_keep_error_ordering(
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = catalog(&["parent"]);
    let mut definitions = SubagentDefinitionSnapshot::default();
    definitions
        .roles
        .insert("role-only".into(), SubagentRole::default());
    let mut settings = SubagentRuntimeConfig::default();
    settings.toggle.insert("role-only".into(), false);
    let allowed = vec!["plan".into()];
    let mut context = parent_context(&definitions, &catalog, &[]);
    context.allowed_types = Some(&allowed);
    let omitted = SubagentDefinitionRequest::default();
    assert_eq!(
        resolve_subagent_definition(&settings, &omitted, &context)?.subagent_type,
        "plan"
    );
    let explicit = SubagentDefinitionRequest {
        subagent_type: "general-purpose".into(),
        type_specified: true,
        ..SubagentDefinitionRequest::default()
    };
    assert!(matches!(
        resolve_subagent_definition(&settings, &explicit, &context),
        Err(SubagentResolutionError::NotAllowed { .. })
    ));
    let missing = SubagentDefinitionRequest {
        subagent_type: "role-only".into(),
        type_specified: true,
        ..SubagentDefinitionRequest::default()
    };
    assert!(matches!(
        resolve_subagent_definition(&settings, &missing, &context),
        Err(SubagentResolutionError::Unknown { .. })
    ));
    settings.toggle.insert("general-purpose".into(), false);
    assert!(matches!(
        resolve_subagent_definition(&settings, &explicit, &context),
        Err(SubagentResolutionError::Disabled { .. })
    ));
    let mut public_model = explicit;
    public_model.model = Some("invalid".into());
    context.model_selection = SubagentModelSelection::Inherited;
    assert_eq!(
        resolve_subagent_definition(&settings, &public_model, &context),
        Err(SubagentResolutionError::HiddenModelSelection)
    );
    public_model.resume = true;
    assert!(matches!(
        resolve_subagent_definition(&settings, &public_model, &context),
        Err(SubagentResolutionError::Disabled { .. })
    ));
    Ok(())
}

#[test]
fn resolved_runtime_intersects_capabilities_and_preserves_definition_metadata(
) -> Result<(), Box<dyn std::error::Error>> {
    use SubagentToolKind as Kind;
    let catalog = catalog(&["parent", "configured", "role", "persona"]);
    let tools: Vec<_> = [
        ("read", Some(Kind::Read)),
        ("edit", Some(Kind::Edit)),
        ("bash", Some(Kind::Execute)),
        ("spawn_subagent", Some(Kind::Task)),
        (
            "get_command_or_subagent_output",
            Some(Kind::BackgroundTaskAction),
        ),
        ("ask_user_question", None),
        ("send_feedback", Some(Kind::Feedback)),
        ("native:workflow", None),
        ("native:question", None),
        ("feedback", None),
        ("custom", None),
        ("mcp:A:search", None),
        ("mcp:A:workflow", None),
        ("mcp:A:question", None),
        ("mcp:A:feedback", None),
        ("mcp:B:search", None),
        ("mcp:B:workflow", None),
    ]
    .into_iter()
    .map(|(id, kind)| SubagentTool {
        id: id.into(),
        kind,
        background_capable: id == "bash",
        mcp_server: id
            .strip_prefix("mcp:")
            .and_then(|id| id.split_once(':'))
            .map(|(name, _)| name.to_owned()),
    })
    .collect();
    let mut definitions = SubagentDefinitionSnapshot::default();
    definitions.project.insert(
        "worker".into(),
        SubagentDefinition {
            name: "worker".into(),
            capability_mode: Some(SubagentCapabilityMode::Execute),
            isolation: Some(SubagentIsolationMode::Worktree),
            max_turns: NonZeroU32::new(5),
            mcp_inheritance: SubagentMcpInheritance::Named {
                named: vec!["A".into()],
            },
            source: SubagentDefinitionSource::Project,
            ..SubagentDefinition::default()
        },
    );
    definitions.roles.insert(
        "worker".into(),
        SubagentRole {
            model: Some("role".into()),
            default_capability_mode: Some("read-write".into()),
            reasoning_effort: Some("high".into()),
            prompt_file: Some("missing".into()),
            ..SubagentRole::default()
        },
    );
    definitions.personas.insert(
        "reviewer".into(),
        SubagentPersona {
            instructions: Some("inline".into()),
            instructions_file: Some("persona.md".into()),
            model: Some("persona".into()),
            ..SubagentPersona::default()
        },
    );
    definitions
        .prompt_files
        .insert(PathBuf::from("/workspace/persona.md"), Ok("file".into()));
    let mut settings = SubagentRuntimeConfig::default();
    settings.models.insert("worker".into(), "configured".into());
    let servers = vec!["A".into(), "B".into()];
    let skills = vec!["review".into()];
    let mut context = parent_context(&definitions, &catalog, &tools);
    context.parent_mcp_servers = &servers;
    context.parent_skills = &skills;
    let request = SubagentDefinitionRequest {
        subagent_type: "worker".into(),
        type_specified: true,
        persona: Some("reviewer".into()),
        isolation: Some(SubagentIsolationMode::None),
        capability_mode: Some(SubagentCapabilityMode::All),
        ..SubagentDefinitionRequest::default()
    };
    let resolved = resolve_subagent_definition(&settings, &request, &context)?;
    assert_eq!(
        (
            resolved.model.as_str(),
            resolved.reasoning_effort.as_deref()
        ),
        ("role", Some("high"))
    );
    assert_eq!(resolved.capability_mode, SubagentCapabilityMode::ReadOnly);
    assert_eq!(resolved.isolation, SubagentIsolationMode::Worktree);
    assert_eq!(resolved.max_turns, NonZeroU32::new(5));
    assert_eq!(
        resolved.persona_instructions.as_deref(),
        Some("inline\n\nfile")
    );
    assert_eq!(resolved.inherited_mcp_servers, ["A"]);
    assert_eq!(resolved.inherited_skills, skills);
    assert_eq!(
        resolved
            .tools
            .iter()
            .map(|tool| tool.id.as_str())
            .collect::<Vec<_>>(),
        [
            "read",
            "custom",
            "mcp:A:search",
            "mcp:A:workflow",
            "mcp:A:question",
            "mcp:A:feedback",
        ]
    );
    assert_eq!(resolved.warnings.len(), 1);
    for builtin in ["explore", "plan"] {
        let request = SubagentDefinitionRequest {
            subagent_type: builtin.into(),
            type_specified: true,
            ..SubagentDefinitionRequest::default()
        };
        let resolved = resolve_subagent_definition(&settings, &request, &context)?;
        assert!(resolved.inherited_skills.is_empty());
        assert!(!resolved
            .tools
            .iter()
            .any(|tool| matches!(tool.kind, Some(Kind::Execute | Kind::Edit | Kind::Task))));
        assert_eq!(resolved.permission_mode, SubagentPermissionMode::Plan);
    }
    let mut unrestricted = definitions.clone();
    unrestricted.roles.clear();
    unrestricted
        .project
        .get_mut("worker")
        .ok_or("worker missing")?
        .capability_mode = None;
    for (mode, edit, execute) in [
        (SubagentCapabilityMode::ReadOnly, false, false),
        (SubagentCapabilityMode::ReadWrite, true, false),
        (SubagentCapabilityMode::Execute, false, true),
        (SubagentCapabilityMode::All, true, true),
    ] {
        let request = SubagentDefinitionRequest {
            subagent_type: "worker".into(),
            type_specified: true,
            capability_mode: Some(mode),
            ..SubagentDefinitionRequest::default()
        };
        let resolved = resolve_subagent_definition(
            &settings,
            &request,
            &parent_context(&unrestricted, &catalog, &tools),
        )?;
        assert_eq!(
            resolved
                .tools
                .iter()
                .any(|tool| tool.kind == Some(Kind::Edit)),
            edit
        );
        assert_eq!(
            resolved
                .tools
                .iter()
                .any(|tool| tool.kind == Some(Kind::Execute)),
            execute
        );
    }
    let denied = vec!["bash".into(), "mcp:A:feedback".into()];
    let mut clamped = parent_context(&unrestricted, &catalog, &tools);
    clamped.operator_denied_tools = &denied;
    clamped.parent_mcp_servers = &servers;
    let request = SubagentDefinitionRequest {
        subagent_type: "worker".into(),
        type_specified: true,
        ..SubagentDefinitionRequest::default()
    };
    let resolved = resolve_subagent_definition(&settings, &request, &clamped)?;
    assert!(!resolved.tools.iter().any(|tool| matches!(
        tool.kind,
        Some(Kind::Execute | Kind::Task | Kind::BackgroundTaskAction)
    )));
    assert!(resolved
        .tools
        .iter()
        .any(|tool| tool.id == "mcp:A:workflow"));
    assert!(!resolved
        .tools
        .iter()
        .any(|tool| tool.id == "mcp:A:feedback" || tool.id == "mcp:B:workflow"));
    Ok(())
}

#[test]
fn persona_failures_abort_but_role_prompts_degrade() -> Result<(), Box<dyn std::error::Error>> {
    let mut definitions = SubagentDefinitionSnapshot::default();
    definitions.roles.insert(
        "general-purpose".into(),
        SubagentRole {
            prompt_file: Some("missing.md".into()),
            model: Some("unknown".into()),
            ..SubagentRole::default()
        },
    );
    definitions
        .personas
        .insert("empty".into(), SubagentPersona::default());
    definitions.personas.insert(
        "broken".into(),
        SubagentPersona {
            instructions: Some("valid inline".into()),
            instructions_file: Some("missing.md".into()),
            ..SubagentPersona::default()
        },
    );
    definitions.prompt_files.insert(
        PathBuf::from("/workspace/missing.md"),
        Err("not found".into()),
    );
    let catalog = catalog(&["parent", "configured"]);
    let mut settings = SubagentRuntimeConfig::default();
    settings
        .models
        .insert("general-purpose".into(), "configured".into());
    let context = parent_context(&definitions, &catalog, &[]);
    let resolved =
        resolve_subagent_definition(&settings, &SubagentDefinitionRequest::default(), &context)?;
    assert_eq!(resolved.model, "configured");
    assert!(resolved.role_prompt.is_none());
    assert_eq!(resolved.warnings.len(), 2);
    for name in ["missing", "empty", "broken"] {
        let request = SubagentDefinitionRequest {
            persona: Some(name.into()),
            ..SubagentDefinitionRequest::default()
        };
        assert!(matches!(
            resolve_subagent_definition(&settings, &request, &context),
            Err(SubagentResolutionError::Persona(_))
        ));
    }
    Ok(())
}
