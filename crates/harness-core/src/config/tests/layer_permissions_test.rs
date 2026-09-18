use super::*;

fn lower_layer_config(model_section: &str, permission: &str) -> String {
    format!(
        r#"{{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              }},
              models: {{
                "gpt-4o-mini": {{ name: "GPT-4o mini" }},
              }},
            }},
          }},
          {model_section}
          permission: {permission},
          runtime: {{ session_dir: "lower-sessions" }},
        }}"#
    )
}

#[test]
fn layer_permissions_sparse_overlays_preserve_inherited_policy() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();

    struct Case {
        name: &'static str,
        lower_permission: &'static str,
        upper_content: &'static str,
        expect_shell: PermissionMode,
        expect_edit: PermissionMode,
    }

    let model_section = r#"model: "default/gpt-4o-mini","#;
    let cases = [
        Case {
            name: "model_only_overlay_keeps_denies",
            lower_permission: r#"{ bash: "deny", edit: "deny" }"#,
            upper_content: r#"{ model: "default/gpt-4o-mini" }"#,
            expect_shell: PermissionMode::Deny,
            expect_edit: PermissionMode::Deny,
        },
        Case {
            name: "explicit_bash_allow_overrides_only_shell",
            lower_permission: r#"{ bash: "deny", edit: "deny" }"#,
            upper_content: r#"{ model: "default/gpt-4o-mini", permission: { bash: "allow" } }"#,
            expect_shell: PermissionMode::Allow,
            expect_edit: PermissionMode::Deny,
        },
        Case {
            name: "permission_only_overlay_inherits_model",
            lower_permission: r#"{ bash: "deny", edit: "deny" }"#,
            upper_content: r#"{ permission: { bash: "ask" } }"#,
            expect_shell: PermissionMode::Ask,
            expect_edit: PermissionMode::Deny,
        },
        Case {
            name: "scalar_permission_mode_covers_all_kinds",
            lower_permission: r#"{ bash: "deny", edit: "deny" }"#,
            upper_content: r#"{ permission: "ask" }"#,
            expect_shell: PermissionMode::Ask,
            expect_edit: PermissionMode::Ask,
        },
        Case {
            name: "fallback_star_pairs_with_explicit_override",
            lower_permission: r#"{ bash: "deny", edit: "deny" }"#,
            upper_content: r#"{ permission: { "*": "ask", bash: "allow" } }"#,
            expect_shell: PermissionMode::Allow,
            expect_edit: PermissionMode::Ask,
        },
        Case {
            name: "upper_legacy_shell_alias_wins_over_lower_canonical",
            lower_permission: r#"{ bash: "deny", edit: "deny" }"#,
            upper_content: r#"{ permission: { shell: "allow" } }"#,
            expect_shell: PermissionMode::Allow,
            expect_edit: PermissionMode::Deny,
        },
        Case {
            name: "upper_canonical_bash_wins_over_lower_legacy_alias",
            lower_permission: r#"{ shell: "deny", edit: "deny" }"#,
            upper_content: r#"{ permission: { bash: "allow" } }"#,
            expect_shell: PermissionMode::Allow,
            expect_edit: PermissionMode::Deny,
        },
    ];

    for case in cases {
        let temp = tempfile::tempdir().unwrap_or_abort();
        let xdg_root = temp.path().join("xdg");
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
        fs::write(
            &xdg_config,
            lower_layer_config(model_section, case.lower_permission),
        )
        .unwrap_or_abort();

        let mut context = discovery_context(temp.path(), Some(&xdg_root));
        context.runtime_content = Some(case.upper_content.to_string());
        let loaded = load_resolved_config_with_context(None, &context)
            .unwrap_or_else(|error| panic!("case {} should load: {error}", case.name))
            .unwrap_or_abort();

        assert_eq!(
            loaded.config.permissions.defaults.shell, case.expect_shell,
            "case {}: resolved shell default",
            case.name
        );
        assert_eq!(
            loaded.config.permissions.defaults.edit, case.expect_edit,
            "case {}: resolved edit default",
            case.name
        );
        assert_eq!(
            loaded.config.runtime.session_dir,
            PathBuf::from("lower-sessions"),
            "case {}: unrelated runtime value must survive the overlay",
            case.name
        );
    }
}

#[test]
fn layer_permissions_permission_only_project_file_loads() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    let cwd_config = temp.path().join("harness.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &xdg_config,
        lower_layer_config(
            r#"model: "default/gpt-4o-mini","#,
            r#"{ bash: "deny", edit: "deny" }"#,
        ),
    )
    .unwrap_or_abort();
    fs::write(&cwd_config, r#"{ permission: { bash: "ask" } }"#).unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));
    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("permission-only layer should load: {error}"))
        .unwrap_or_abort();

    assert_eq!(loaded.paths, vec![xdg_config.clone(), cwd_config.clone()]);
    assert!(matches!(
        loaded.config.permissions.defaults.shell,
        PermissionMode::Ask
    ));
    assert!(matches!(
        loaded.config.permissions.defaults.edit,
        PermissionMode::Deny
    ));
    assert_eq!(
        loaded.config.runtime.session_dir,
        PathBuf::from("lower-sessions")
    );
    assert!(
        loaded.config.agents.contains_key("default"),
        "shipped profiles must build from the inherited model"
    );
}

#[test]
fn layer_permissions_missing_model_in_all_layers_still_fails() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");

    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &xdg_config,
        lower_layer_config("", r#"{ bash: "deny", edit: "deny" }"#),
    )
    .unwrap_or_abort();

    let mut context = discovery_context(temp.path(), Some(&xdg_root));
    context.runtime_content = Some(r#"{ permission: { bash: "ask" } }"#.to_string());
    let error = load_resolved_config_with_context(None, &context)
        .expect_err("merged config without any model must fail");
    assert!(
        error.to_string().contains("top-level `model` is required"),
        "unexpected error: {error}"
    );
}

#[test]
fn layer_permissions_file_references_resolve_per_layer_directory() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    fs::create_dir_all(xdg_root.join("harness/keys")).unwrap_or_abort();
    fs::write(xdg_root.join("harness/keys/a.txt"), "key-a").unwrap_or_abort();
    fs::write(
        &xdg_config,
        r#"{
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "{file:keys/a.txt}",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" },
              },
            },
          },
          model: "default/gpt-4o-mini",
          permission: { bash: "deny", edit: "deny" },
        }"#,
    )
    .unwrap_or_abort();

    let cwd_config = temp.path().join("harness.jsonc");
    fs::create_dir_all(temp.path().join("keys")).unwrap_or_abort();
    fs::write(temp.path().join("keys/b.txt"), "key-b").unwrap_or_abort();
    fs::write(
        &cwd_config,
        r#"{
          provider: {
            extra: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "{file:keys/b.txt}",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" },
              },
            },
          },
        }"#,
    )
    .unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));
    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("layered file references should load: {error}"))
        .unwrap_or_abort();

    let ProviderConfig::OpenAiCompatible(default) =
        loaded.config.providers.get("default").unwrap_or_abort()
    else {
        panic!("expected OpenAiCompatible default provider");
    };
    assert_eq!(
        default.api_key, "key-a",
        "lower-layer file reference must resolve against its own directory"
    );
    let ProviderConfig::OpenAiCompatible(extra) =
        loaded.config.providers.get("extra").unwrap_or_abort()
    else {
        panic!("expected OpenAiCompatible extra provider");
    };
    assert_eq!(
        extra.api_key, "key-b",
        "upper-layer file reference must resolve against its own directory"
    );
}

#[test]
fn layer_permissions_instructions_accumulate_in_layer_order() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &xdg_config,
        r#"{
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" },
              },
            },
          },
          model: "default/gpt-4o-mini",
          permission: { bash: "deny", edit: "deny" },
          instructions: ["first"],
        }"#,
    )
    .unwrap_or_abort();

    let cwd_config = temp.path().join("harness.jsonc");
    fs::write(&cwd_config, r#"{ instructions: ["second"] }"#).unwrap_or_abort();

    let mut context = discovery_context(temp.path(), Some(&xdg_root));
    context.runtime_content = Some(r#"{ instructions: ["third"] }"#.to_string());
    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("layered instructions should load: {error}"))
        .unwrap_or_abort();

    let contents: Vec<&str> = loaded
        .config
        .instruction_files
        .iter()
        .map(|file| file.content.as_str())
        .collect();
    assert_eq!(
        contents,
        vec!["first", "second", "third"],
        "configured instructions must accumulate in layer order"
    );
}

#[test]
fn layer_permissions_omitted_formatter_survives_and_arrays_replace() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config = xdg_root.join("harness/harness.jsonc");
    fs::create_dir_all(xdg_config.parent().unwrap_or_abort()).unwrap_or_abort();
    fs::write(
        &xdg_config,
        r#"{
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" },
              },
            },
          },
          model: "default/gpt-4o-mini",
          permission: { bash: "deny", edit: "deny" },
          formatter: { enabled: false },
          skills: { disabled: ["skill-a"] },
        }"#,
    )
    .unwrap_or_abort();

    let cwd_config = temp.path().join("harness.jsonc");
    fs::write(&cwd_config, r#"{ skills: { disabled: ["skill-b"] } }"#).unwrap_or_abort();

    let context = discovery_context(temp.path(), Some(&xdg_root));
    let loaded = load_resolved_config_with_context(None, &context)
        .unwrap_or_else(|error| panic!("layered formatter and skills should load: {error}"))
        .unwrap_or_abort();

    assert!(
        !loaded.config.formatter.enabled,
        "formatter omitted from the later layer must survive the merge"
    );
    assert_eq!(
        loaded.config.skills.disabled,
        vec!["skill-b".to_string()],
        "array values must replace earlier layers, not concatenate"
    );
}
