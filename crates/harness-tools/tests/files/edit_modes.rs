use super::*;

#[tokio::test]
async fn model_edit_formats_change_catalog_and_reads() -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::{
        agent::AgentProfile,
        config::{EditConfig, EditMode},
    };
    for (model, mode) in [
        ("mock:claude", EditMode::Hashline),
        ("mock:kimi-k2", EditMode::Replace),
        ("mock:gpt-6", EditMode::ApplyPatch),
    ] {
        let temp = tempfile::tempdir()?;
        fs::write(temp.path().join("sample.txt"), "alpha\nbeta\n")?;
        let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
        registry.set_edit_config(EditConfig::default());
        let mut profile = AgentProfile::fallback("default");
        profile.model_ref = model.into();
        profile.toolset = ["read", "edit", "write", "apply_patch"]
            .map(str::to_owned)
            .into();
        let policy = PermissionPolicy::from_rules(vec![PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        }])?;
        let definitions = registry.prompt_definitions(&profile, None, model, (&policy, &policy));
        let edit = definitions.iter().find(|tool| tool.tool_id == "edit");
        assert_eq!(edit.is_some(), mode != EditMode::ApplyPatch);
        assert_eq!(
            definitions.iter().any(|tool| tool.tool_id == "write"),
            mode != EditMode::ApplyPatch
        );
        assert_eq!(
            definitions.iter().any(|tool| tool.tool_id == "apply_patch"),
            mode == EditMode::ApplyPatch
        );
        if let Some(edit) = edit {
            assert_eq!(
                edit.parameters["properties"].get("edits").is_some(),
                mode == EditMode::Hashline
            );
        }
        let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
        config.tool_registry = Arc::new(registry);
        config.permission_policy = policy;
        config.agent_profiles.insert("default".into(), profile);
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("edit modes", temp.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let result = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::Worker, Some(agent)),
                None,
                "read",
                json!({"path":"sample.txt"}),
            )
            .await?;
        if mode == EditMode::Hashline {
            assert!(result.display_text.starts_with("1#"));
            assert!(result.display_text.contains("|alpha\n2#"));
        } else {
            assert_eq!(result.display_text, "alpha\nbeta\n");
        }
        coordinator.stop_run().await?;
    }
    Ok(())
}
