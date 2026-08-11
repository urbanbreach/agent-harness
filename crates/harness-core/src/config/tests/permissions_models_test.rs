use super::*;
use crate::perm::{PermissionKind, PermissionPolicy, PolicyDecision};
use crate::UnwrapOrAbort;

#[test]
fn oc_parity_permission_omitted_defaults_to_allow_for_bash_edit_webfetch() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_without_permission();
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();
    let policy = PermissionPolicy::from_config(&parsed);

    assert_eq!(
        parsed.permissions.defaults.edit,
        PermissionMode::Allow,
        "expected Allow for edit on default config (permission omitted), got {:?}",
        parsed.permissions.defaults.edit
    );
    assert_eq!(
        parsed.permissions.defaults.shell,
        PermissionMode::Allow,
        "expected Allow for bash/shell on default config (permission omitted), got {:?}",
        parsed.permissions.defaults.shell
    );
    assert_eq!(
        parsed.permissions.defaults.webfetch,
        Some(PermissionMode::Allow),
        "expected Allow for webfetch on default config (permission omitted), got {:?}",
        parsed.permissions.defaults.webfetch
    );
    assert_eq!(
        parsed.permissions.defaults.external_directory,
        Some(PermissionMode::Ask),
        "expected Ask for external_directory when permission omitted, got {:?}",
        parsed.permissions.defaults.external_directory
    );
    assert_eq!(
        parsed.permissions.defaults.doom_loop,
        Some(PermissionMode::Ask),
        "expected Ask for doom_loop when permission omitted, got {:?}",
        parsed.permissions.defaults.doom_loop
    );
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Deny),
        "expected Deny for base question when permission omitted, got {:?}",
        parsed.permissions.defaults.question
    );

    for (kind, label) in [
        (PermissionKind::EditFs, "edit"),
        (PermissionKind::Shell, "bash"),
        (PermissionKind::WebFetch, "webfetch"),
    ] {
        let decision = policy.evaluate(None, kind);
        assert!(
            matches!(decision, PolicyDecision::Allow),
            "expected Allow for {label} on default config, got {decision:?}"
        );
    }
    assert!(
        matches!(
            policy.evaluate(None, PermissionKind::ExternalDirectory),
            PolicyDecision::Ask { .. }
        ),
        "expected Ask for external_directory when permission omitted"
    );
    assert!(
        matches!(
            policy.evaluate(None, PermissionKind::DoomLoop),
            PolicyDecision::Ask { .. }
        ),
        "expected Ask for doom_loop when permission omitted"
    );
    assert!(
        matches!(
            policy.evaluate(None, PermissionKind::Question),
            PolicyDecision::Deny
        ),
        "expected Deny for question when permission omitted"
    );
}

#[test]
fn oc_parity_permission_allow_scalar_expands_without_forcing_ask() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission("\"allow\"");
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();
    let policy = PermissionPolicy::from_config(&parsed);

    assert_eq!(
        parsed.permissions.defaults.edit,
        PermissionMode::Allow,
        "expected Allow for edit when permission scalar is allow, got {:?}",
        parsed.permissions.defaults.edit
    );
    assert_eq!(
        parsed.permissions.defaults.shell,
        PermissionMode::Allow,
        "expected Allow for bash when permission scalar is allow, got {:?}",
        parsed.permissions.defaults.shell
    );
    assert_eq!(
        parsed.permissions.defaults.webfetch,
        Some(PermissionMode::Allow),
        "expected Allow for webfetch when permission scalar is allow, got {:?}",
        parsed.permissions.defaults.webfetch
    );
    // Scalar allow keeps safety exceptions (not YOLO on external/doom/question).
    assert_eq!(
        parsed.permissions.defaults.external_directory,
        Some(PermissionMode::Ask),
        "expected Ask for external_directory when permission scalar is allow, got {:?}",
        parsed.permissions.defaults.external_directory
    );
    assert_eq!(
        parsed.permissions.defaults.doom_loop,
        Some(PermissionMode::Ask),
        "expected Ask for doom_loop when permission scalar is allow, got {:?}",
        parsed.permissions.defaults.doom_loop
    );
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Deny),
        "expected Deny for question when permission scalar is allow, got {:?}",
        parsed.permissions.defaults.question
    );

    for (kind, label) in [
        (PermissionKind::EditFs, "edit"),
        (PermissionKind::Shell, "bash"),
        (PermissionKind::WebFetch, "webfetch"),
    ] {
        let decision = policy.evaluate(None, kind);
        assert!(
            matches!(decision, PolicyDecision::Allow),
            "expected Allow for {label} when permission scalar is allow, got {decision:?}"
        );
        assert!(
            !matches!(decision, PolicyDecision::Ask { .. }),
            "expected Allow for {label} (not Ask) when permission scalar is allow, got {decision:?}"
        );
    }
    assert!(
        matches!(
            policy.evaluate(None, PermissionKind::ExternalDirectory),
            PolicyDecision::Ask { .. }
        ),
        "expected Ask for external_directory when permission scalar is allow"
    );
    assert!(
        matches!(
            policy.evaluate(None, PermissionKind::DoomLoop),
            PolicyDecision::Ask { .. }
        ),
        "expected Ask for doom_loop when permission scalar is allow"
    );
    assert!(
        matches!(
            policy.evaluate(None, PermissionKind::Question),
            PolicyDecision::Deny
        ),
        "expected Deny for question when permission scalar is allow"
    );
}

#[test]
fn oc_parity_example_config_permission_scalar_is_allow_not_ask_all() {
    // arrange
    // act
    // assert
    let example_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../configs/harness.example.jsonc");
    let raw = std::fs::read_to_string(&example_path).unwrap_or_abort();
    assert!(
        raw.contains(r#""permission": "allow""#) || raw.contains("permission: \"allow\""),
        "expected configs/harness.example.jsonc permission scalar to be allow (Harness default), not ask-all; found ask or missing allow"
    );

    let context = ConfigLoadContext::from_env()
        .with_current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let parsed = load_config_from_file_with_context(&example_path, &context).unwrap_or_abort();
    assert_eq!(
        parsed.permissions.defaults.edit,
        PermissionMode::Allow,
        "expected example config edit default Allow after allow scalar, got {:?}",
        parsed.permissions.defaults.edit
    );
    assert_eq!(
        parsed.permissions.defaults.shell,
        PermissionMode::Allow,
        "expected example config bash default Allow after allow scalar, got {:?}",
        parsed.permissions.defaults.shell
    );
    assert!(
        !matches!(parsed.permissions.defaults.edit, PermissionMode::Ask),
        "expected example config not to force ask-all for edit"
    );
    assert_eq!(
        parsed.permissions.defaults.external_directory,
        Some(PermissionMode::Ask),
        "expected example scalar allow to keep external_directory Ask, got {:?}",
        parsed.permissions.defaults.external_directory
    );
    assert_eq!(
        parsed.permissions.defaults.doom_loop,
        Some(PermissionMode::Ask),
        "expected example scalar allow to keep doom_loop Ask, got {:?}",
        parsed.permissions.defaults.doom_loop
    );
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Deny),
        "expected example scalar allow to keep base question Deny, got {:?}",
        parsed.permissions.defaults.question
    );
}

#[test]
fn permission_scalar_expands_to_public_kinds_and_network() {
    // arrange
    // act
    // assert
    // Scalar ask/deny paint every kind; scalar allow keeps safety exceptions.
    for (raw, mode) in [
        ("\"ask\"", PermissionMode::Ask),
        ("\"deny\"", PermissionMode::Deny),
    ] {
        let cfg = public_minimal_config_with_permission(raw);
        let parsed = load_config_from_str(&cfg).unwrap_or_abort();
        assert_eq!(parsed.permissions.defaults.edit, mode);
        assert_eq!(parsed.permissions.defaults.shell, mode);
        assert_eq!(parsed.permissions.defaults.network, mode);
        assert_eq!(parsed.permissions.defaults.question, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.task, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.webfetch, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.websearch, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.codesearch, Some(mode.clone()));
        assert_eq!(parsed.permissions.defaults.lsp, Some(mode.clone()));
        assert_eq!(
            parsed.permissions.defaults.external_directory,
            Some(mode.clone())
        );
        assert_eq!(parsed.permissions.defaults.doom_loop, Some(mode));
    }

    let cfg = public_minimal_config_with_permission("\"allow\"");
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();
    assert_eq!(parsed.permissions.defaults.edit, PermissionMode::Allow);
    assert_eq!(parsed.permissions.defaults.shell, PermissionMode::Allow);
    assert_eq!(parsed.permissions.defaults.network, PermissionMode::Allow);
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Deny)
    );
    assert_eq!(
        parsed.permissions.defaults.task,
        Some(PermissionMode::Allow)
    );
    assert_eq!(
        parsed.permissions.defaults.webfetch,
        Some(PermissionMode::Allow)
    );
    assert_eq!(
        parsed.permissions.defaults.websearch,
        Some(PermissionMode::Allow)
    );
    assert_eq!(
        parsed.permissions.defaults.codesearch,
        Some(PermissionMode::Allow)
    );
    assert_eq!(parsed.permissions.defaults.lsp, Some(PermissionMode::Allow));
    assert_eq!(
        parsed.permissions.defaults.external_directory,
        Some(PermissionMode::Ask)
    );
    assert_eq!(
        parsed.permissions.defaults.doom_loop,
        Some(PermissionMode::Ask)
    );
}

#[test]
fn permission_scalar_rejects_invalid_mode() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission("\"maybe\"");
    load_config_from_str(&cfg).expect_err("invalid permission scalar must fail");
}

#[test]
fn permission_object_accepts_per_tool_scalar_modes() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission(
        r#"{
                bash: "ask",
                edit: "deny",
                question: "allow",
                task: "ask",
                webfetch: "deny",
                websearch: "allow",
                codesearch: "deny",
                lsp: "allow"
            }"#,
    );
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(parsed.permissions.defaults.shell, PermissionMode::Ask);
    assert_eq!(parsed.permissions.defaults.edit, PermissionMode::Deny);
    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Allow)
    );
    assert_eq!(parsed.permissions.defaults.task, Some(PermissionMode::Ask));
    assert_eq!(
        parsed.permissions.defaults.webfetch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(
        parsed.permissions.defaults.websearch,
        Some(PermissionMode::Allow)
    );
    assert_eq!(
        parsed.permissions.defaults.codesearch,
        Some(PermissionMode::Deny)
    );
    assert_eq!(parsed.permissions.defaults.lsp, Some(PermissionMode::Allow));
    assert!(parsed.permissions.rules.shell.is_empty());
    assert!(parsed.permissions.rules.edit.is_empty());
    assert!(parsed.permissions.rules.task.is_empty());
}

#[test]
fn permission_object_accepts_read_external_directory_and_doom_loop_keys() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission(
        r#"{
                read: {
                  "*.env": "ask",
                  "*.env.example": "allow"
                },
                external_directory: "ask",
                doom_loop: "ask"
            }"#,
    );
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();
    let policy = PermissionPolicy::from_config(&parsed);

    assert_eq!(
        parsed.permissions.defaults.external_directory,
        Some(PermissionMode::Ask)
    );
    assert_eq!(
        parsed.permissions.defaults.doom_loop,
        Some(PermissionMode::Ask)
    );
    assert_eq!(parsed.permissions.rules.read.len(), 2);
    assert!(parsed.permissions.rules.external_directory.is_empty());
    assert!(matches!(
        policy.evaluate(None, PermissionKind::ExternalDirectory),
        PolicyDecision::Ask { .. }
    ));
    assert!(matches!(
        policy.evaluate(None, PermissionKind::DoomLoop),
        PolicyDecision::Ask { .. }
    ));
    assert_eq!(
        policy.evaluate_request(
            None,
            PermissionKind::Read,
            Some(&crate::perm::PermissionRuleRequest::WorkspacePath(
                "foo.env".to_string()
            )),
        ),
        PolicyDecision::Ask {
            timeout_ms: 0,
            default_decision: crate::perm::PermissionDecision::Deny,
        }
    );
}

#[test]
fn permission_rule_object_preserves_shell_allowlist_and_rules() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission(
        r#"{
                "*": "deny",
                bash: {
                  "git status": "allow",
                  "cargo test*": "ask",
                  "*": "deny"
                },
                edit: {
                  "docs/**": "allow",
                  "crates/harness-core/src/config.rs": "ask",
                  "*": "deny"
                },
                task: {
                  "explore": "allow",
                  "review-*": "ask",
                  "*": "deny"
                },
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."]
                }
            }"#,
    );
    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(
        parsed.permissions.defaults.question,
        Some(PermissionMode::Deny)
    );
    assert_eq!(parsed.permissions.shell_allowlist.executables, vec!["git"]);
    assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["."]);
    assert_eq!(parsed.permissions.rules.shell.len(), 3);
    assert_eq!(parsed.permissions.rules.edit.len(), 3);
    assert_eq!(parsed.permissions.rules.task.len(), 3);
}

#[test]
fn shell_allowlist_loads_legacy_flat_shape_with_default_mode() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission(
        r#"{
                bash: "allow",
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."]
                }
            }"#,
    );

    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(
        parsed.permissions.shell_allowlist.mode,
        ShellAllowlistMode::PermissionPatterns
    );
    assert_eq!(parsed.permissions.shell_allowlist.executables, vec!["git"]);
    assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["."]);
}

#[test]
fn shell_allowlist_accepts_camel_case_cwd_roots_and_policy_mode() {
    // arrange
    // act
    // assert
    let cfg = public_minimal_config_with_permission(
        r#"{
                bash: "allow",
                shellAllowlist: {
                  executables: ["cargo"],
                  cwdRoots: ["crates"],
                  policy_mode: "legacy_executables"
                }
            }"#,
    );

    let parsed = load_config_from_str(&cfg).unwrap_or_abort();

    assert_eq!(
        parsed.permissions.shell_allowlist.mode,
        ShellAllowlistMode::LegacyExecutables
    );
    assert_eq!(
        parsed.permissions.shell_allowlist.executables,
        vec!["cargo"]
    );
    assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["crates"]);
}

#[test]
fn shell_allowlist_mode_round_trips_through_json() {
    // arrange
    // act
    // assert
    let allowlist = ShellAllowlist {
        mode: ShellAllowlistMode::LegacyExecutables,
        executables: vec!["git".to_string()],
        cwd_roots: vec![".".to_string()],
    };

    let json = serde_json::to_value(&allowlist).unwrap_or_abort();
    assert_eq!(
        json.get("mode"),
        Some(&serde_json::json!("legacy_executables"))
    );

    let parsed: ShellAllowlist = serde_json::from_value(json).unwrap_or_abort();
    assert_eq!(parsed.mode, ShellAllowlistMode::LegacyExecutables);
    assert_eq!(parsed.executables, vec!["git"]);
    assert_eq!(parsed.cwd_roots, vec!["."]);

    let camel_case_alias: ShellAllowlist = serde_json::from_value(serde_json::json!({
        "policyMode": "legacy_executables",
    }))
    .unwrap_or_abort();
    assert_eq!(camel_case_alias.mode, ShellAllowlistMode::LegacyExecutables);
}

#[test]
fn permission_rule_rejects_invalid_selector_forms() {
    // arrange
    // act
    // assert
    for permission in [
        r#"{ bash: { "/^git/": "allow" } }"#,
        r#"{ bash: { "cargo * test": "allow" } }"#,
        r#"{ edit: { "../secrets/**": "allow" } }"#,
        r#"{ edit: { "/tmp/file": "allow" } }"#,
        r#"{ edit: { "docs/*": "allow" } }"#,
        r#"{ bash: { "git status": "sometimes" } }"#,
        r#"{ bash: { "git status": { mode: "allow" } } }"#,
        r#"{ edit: { "docs/**": 1 } }"#,
        r#"{ task: { "/explore/": "allow" } }"#,
        r#"{ question: { "*": "allow" } }"#,
    ] {
        let cfg = public_minimal_config_with_permission(permission);
        load_config_from_str(&cfg).expect_err("invalid permission selector form must fail");
    }
}

#[test]
fn model_limit_modalities_and_options_normalize_to_catalog_metadata() {
    // arrange
    // act
    // assert
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                timeoutMs: 30000
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  limit: { context: 272000, input: 200000, output: 128000 },
                  modalities: { input: ["text", "image"], output: ["text"] },
                  options: { reasoning: { effort: "high" } },
                  variants: {
                    fast: {
                      name: "Fast",
                      limit: { context: 128000, input: 64000, output: 32000 },
                      modalities: { input: ["text"], output: ["text"] },
                      options: { temperature: 0.2 }
                    }
                  }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            default: {
              system_prompt: "Do the work",
              variant: "fast"
            }
          },
          permission: "allow"
        }
        "#;

    let parsed = load_config_from_str(cfg).unwrap_or_abort();
    let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap()
    else {
        panic!("expected OpenAiCompatible");
    };
    assert_eq!(provider.timeout_ms, 30_000);
    let model = &provider.models["gpt-4o-mini"];
    assert_eq!(model.limit.context, Some(272_000));
    assert_eq!(model.modalities.input, vec!["text", "image"]);
    assert!(model.options.contains_key("reasoning"));
    assert_eq!(model.variants["fast"].limit.output, Some(32_000));

    let metadata = resolve_profile_model_metadata(&parsed, "default").unwrap_or_abort();
    assert_eq!(metadata.context_window_tokens, Some(128_000));
    assert_eq!(metadata.max_input_tokens, Some(64_000));
    assert_eq!(metadata.max_output_tokens, Some(32_000));
}

#[test]
fn model_limit_rejects_unknown_metadata_fields() {
    // arrange
    // act
    // assert
    let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  limit: { context: 272000, training: 1 }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          permission: "allow"
        }
        "#;

    let err = load_config_from_str(cfg).expect_err("unknown limit field must fail");
    assert!(
        err.to_string().contains("unknown field `training`"),
        "unexpected error: {err}"
    );
}
