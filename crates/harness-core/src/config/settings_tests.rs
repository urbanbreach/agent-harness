use super::*;

#[test]
fn settings_writes_validate_before_commit_and_keep_raw_references(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("harness.jsonc");
    std::fs::write(
        temp.path().join("key.txt"),
        "sk-private-fixture-keep-out-of-config",
    )?;
    std::fs::write(
        &path,
        r#"{ // authored configuration
        provider: {local:{type:'openai_compatible',base_url:'http://localhost:8000/v1',api_key:'{file:key.txt}',models:{test:{}}}}, model:'local:test',
        hashlineEdit:false, runtime:{compaction:{fallbackInputTokens:1234}},
        permissions:{bash:'ask',edit:'deny',read:{'src/*':'allow','*':'deny','src/private/*':'ask'}},
        agents:{default:{permission:{bash:{'git *':'allow','*':'deny','git status':'ask'}}}}
    }"#,
    )?;
    assert!(!read_effective_hashline_edit(&path)?);
    assert!(write_project_hashline_edit(&path, true)?);
    assert_eq!(
        write_project_setting_value(&path, "runtime.compaction.fallback_input_tokens", "8192")?,
        "8192"
    );
    assert_eq!(
        write_project_setting_value(&path, "permission.bash", "allow")?,
        "allow"
    );
    assert_eq!(
        load_config_from_file(&path)?.permissions.defaults.edit,
        PermissionMode::Deny
    );
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(
        (
            body.contains("{file:key.txt}"),
            body.contains("sk-private-fixture"),
            body.contains("hashlineEdit")
        ),
        (true, false, false)
    );
    let config = load_config_from_file(&path)?;
    for rules in [
        &config.permissions.rules.read,
        &config.agents["default"]
            .permissions
            .as_ref()
            .ok_or("agent permissions missing")?
            .rules
            .shell,
    ] {
        assert_eq!(
            rules.iter().map(|rule| rule.mode).collect::<Vec<_>>(),
            [
                PermissionMode::Allow,
                PermissionMode::Deny,
                PermissionMode::Ask
            ]
        );
    }
    for (id, input) in [
        ("runtime.compaction.fallback_input_tokens", "-1"),
        ("permission.bash", "sometimes"),
        ("runtime.session_dir", ""),
        ("provider.apiKey", "secret"),
        ("unknown.setting", "true"),
        ("confirm_before_rewind", "false"),
    ] {
        assert!(
            write_project_setting_value(&path, id, input).is_err(),
            "{id}"
        );
        assert_eq!(std::fs::read_to_string(&path)?, body, "{id}");
    }
    assert_eq!(
        reset_project_setting_to_default(&path, "runtime.compaction.fallback_input_tokens")?,
        "32768"
    );
    assert_eq!(
        read_project_setting_value(&path, "runtime.compaction.fallback_input_tokens")?.as_deref(),
        None
    );
    assert_eq!(
        load_config_from_file(&path)?
            .runtime
            .compaction
            .fallback_input_tokens,
        32768
    );
    let tui = temp.path().join("tui.jsonc");
    std::fs::write(&tui, "{keybinds:{copy_selection:'ctrl+y'}}")?;
    write_rewind_confirmation(&tui, false)?;
    let parsed: PublicTuiConfig = json5::from_str(&std::fs::read_to_string(&tui)?)?;
    assert_eq!(parsed.confirm_before_rewind, Some(false));
    assert_eq!(parsed.keybindings["copy_selection"], "ctrl+y");
    let context = ConfigDiscoveryContext {
        current_dir: temp.path().into(),
        xdg_config_home: None,
        home: None,
        runtime_config_path: None,
        tui_config_path: None,
    };
    assert_eq!(
        rewind_confirmation_settings(Some(&path), &context)?,
        (false, tui)
    );
    let empty = temp.path().join("no-config");
    std::fs::create_dir(&empty)?;
    let empty_context = context.with_current_dir(empty.clone());
    assert!(rewind_confirmation_settings(None, &empty_context)?.0);
    assert_eq!(std::fs::read_dir(empty)?.count(), 0);
    Ok(())
}
