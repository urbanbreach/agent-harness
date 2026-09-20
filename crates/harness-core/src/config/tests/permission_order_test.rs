use super::*;
use crate::perm::{PermissionKind, PermissionPolicy, PermissionRuleRequest, PolicyDecision};

fn order_fixture(fragment: &str) -> String {
    format!(
        r#"{{
          provider: {{ default: {{
            type: "openai_compatible",
            base_url: "http://127.0.0.1:8317/v1",
            api_key: "test-key",
            models: {{ "gpt-4o-mini": {{ name: "GPT-4o mini" }} }},
          }} }},
          model: "default/gpt-4o-mini",
          {fragment}
        }}"#
    )
}

fn permission_fragment(agent: bool, fields: &str) -> String {
    if agent {
        format!("agent: {{ default: {{ permission: {{ {fields} }} }} }}")
    } else {
        format!("permission: {{ {fields} }}")
    }
}

#[test]
fn permission_order_survives_public_loaders_and_layers() {
    let _lock = CONFIG_DISCOVERY_TEST_LOCK.lock().unwrap_or_abort();
    let temp = tempfile::tempdir().unwrap_or_abort();
    let path = temp.path().join("order.jsonc");
    let context = discovery_context(temp.path(), None);

    for agent in [false, true] {
        for (kind_name, kind, target) in [
            ("bash", PermissionKind::Shell, "git status"),
            ("shell", PermissionKind::Shell, "git status"),
            ("edit", PermissionKind::EditFs, "src/main.rs"),
            ("task", PermissionKind::Task, "general"),
            ("read", PermissionKind::Read, "src/main.rs"),
            (
                "external_directory",
                PermissionKind::ExternalDirectory,
                "/outside/main.rs",
            ),
        ] {
            let request = match kind {
                PermissionKind::Shell => PermissionRuleRequest::ShellCommand {
                    pattern: target.into(),
                },
                PermissionKind::Task => PermissionRuleRequest::TaskAgent(target.into()),
                _ => PermissionRuleRequest::WorkspacePath(target.into()),
            };
            for (rules, expected) in [
                (
                    format!(r#"{{ "{target}": "deny", "*": "allow" }}"#),
                    PolicyDecision::Allow,
                ),
                (
                    format!(r#"{{ "*": "allow", "{target}": "deny" }}"#),
                    PolicyDecision::Deny,
                ),
                (r#""allow""#.to_string(), PolicyDecision::Allow),
            ] {
                let raw = order_fixture(&permission_fragment(
                    agent,
                    &format!("{kind_name}: {rules}"),
                ));
                fs::write(&path, &raw).unwrap_or_abort();
                let mut content_context = context.clone();
                content_context.runtime_content = Some(raw.clone());
                let configs = [
                    load_config_from_str(&raw).unwrap_or_abort(),
                    load_config_from_file(&path).unwrap_or_abort(),
                    load_config_from_file_with_context(&path, &context).unwrap_or_abort(),
                    load_resolved_config_with_context(Some(&path), &context)
                        .unwrap_or_abort()
                        .unwrap_or_abort()
                        .config,
                    load_resolved_config_with_context(None, &content_context)
                        .unwrap_or_abort()
                        .unwrap_or_abort()
                        .config,
                ];
                for (loader, config) in configs.iter().enumerate() {
                    assert_eq!(
                        PermissionPolicy::from_config(config).evaluate_request(
                            agent.then_some("default"),
                            kind,
                            Some(&request)
                        ),
                        expected,
                        "agent={agent} kind={kind_name} loader={loader} rules={rules}",
                    );
                }
            }
        }
    }

    // Exercise file and content overlays together; unrelated denies must remain inherited.
    let xdg = temp.path().join("xdg");
    let lower_path = xdg.join("harness/harness.jsonc");
    let project_path = temp.path().join("harness.jsonc");
    fs::create_dir_all(lower_path.parent().unwrap_or_abort()).unwrap_or_abort();
    for agent in [false, true] {
        for (file_fields, content_fields, expected) in [
            (
                "",
                r#"bash: { "git status": "allow" }"#,
                [PolicyDecision::Allow, PolicyDecision::Deny],
            ),
            (
                r#"shell: { "git status": "allow" }"#,
                "",
                [PolicyDecision::Allow, PolicyDecision::Deny],
            ),
            (
                "",
                r#"question: "allow""#,
                [PolicyDecision::Deny, PolicyDecision::Deny],
            ),
            (
                "",
                r#"bash: "allow""#,
                [PolicyDecision::Allow, PolicyDecision::Allow],
            ),
            (
                r#"shell: "allow""#,
                r#"bash: { "git status": "deny" }"#,
                [PolicyDecision::Deny, PolicyDecision::Allow],
            ),
            (
                r#"bash: { "git status": "deny" }"#,
                r#"shell: { "git status": "deny", "*": "allow" }"#,
                [PolicyDecision::Allow, PolicyDecision::Allow],
            ),
        ] {
            fs::write(
                &lower_path,
                order_fixture(&permission_fragment(
                    agent,
                    r#"bash: { "git status": "allow", "*": "deny" }, edit: "deny""#,
                )),
            )
            .unwrap_or_abort();
            fs::write(
                &project_path,
                format!("{{ {} }}", permission_fragment(agent, file_fields)),
            )
            .unwrap_or_abort();
            let mut context = discovery_context(temp.path(), Some(&xdg));
            context.runtime_content = Some(format!(
                "{{ {} }}",
                permission_fragment(agent, content_fields)
            ));
            let loaded = load_resolved_config_with_context(None, &context)
                .unwrap_or_else(|error| {
                    panic!("agent={agent} file={file_fields} content={content_fields}: {error}")
                })
                .unwrap_or_abort();
            let policy = PermissionPolicy::from_config(&loaded.config);
            for (command, expected) in ["git status", "git diff"].into_iter().zip(expected) {
                assert_eq!(
                    policy.evaluate_request(
                        agent.then_some("default"),
                        PermissionKind::Shell,
                        Some(&PermissionRuleRequest::ShellCommand {
                            pattern: command.into()
                        })
                    ),
                    expected,
                    "agent={agent} command={command} file={file_fields} content={content_fields}"
                );
            }
            assert_eq!(
                policy.evaluate(agent.then_some("default"), PermissionKind::EditFs),
                PolicyDecision::Deny
            );
        }
    }
    fs::remove_file(&project_path).unwrap_or_abort();

    // Duplicate fields retain their last occurrence; raw keys stay distinct until normalization.
    for fragment in [
        r#"permission: { bash: { "git status": "allow", "*": "deny", "git status": "allow" } }"#,
        r#"permission: { bash: { "git status": "deny", " git status ": "allow" } }"#,
        r#"permission: { bash: { "*": "deny" } }, permission: { bash: { "git status": "deny", "*": "allow" } }"#,
        r#"agent: { default: { permission: { bash: { "*": "deny" } } } }, agent: { default: { permissions: { shell: { "git status": "deny", "*": "allow" } } } }"#,
    ] {
        let raw = order_fixture(fragment);
        fs::write(&path, &raw).unwrap_or_abort();
        for config in [
            load_config_from_str(&raw).unwrap_or_abort(),
            load_resolved_config_with_context(Some(&path), &context)
                .unwrap_or_abort()
                .unwrap_or_abort()
                .config,
        ] {
            assert_eq!(
                PermissionPolicy::from_config(&config).evaluate_request(
                    Some("default"),
                    PermissionKind::Shell,
                    Some(&PermissionRuleRequest::ShellCommand {
                        pattern: "git status".into()
                    })
                ),
                PolicyDecision::Allow,
                "{fragment}"
            );
        }
    }

    // Layer-only alias merging must follow the same nesting order for values and metadata.
    for fragment in [
        r#"permission: { shell: { "git status": "deny", "*": "allow" }, bash: { "*": "deny", "git status": "allow" } }"#,
        r#"agent: { default: {
            permission: { bash: { "*": "deny" }, shell: { "git status": "deny", "*": "allow" } },
            permissions: { bash: { "git status": "allow" } }
        } }"#,
    ] {
        fs::write(&path, order_fixture(fragment)).unwrap_or_abort();
        let config = load_resolved_config_with_context(Some(&path), &context)
            .unwrap_or_abort()
            .unwrap_or_abort()
            .config;
        assert_eq!(
            PermissionPolicy::from_config(&config).evaluate_request(
                Some("default"),
                PermissionKind::Shell,
                Some(&PermissionRuleRequest::ShellCommand {
                    pattern: "git status".into()
                })
            ),
            PolicyDecision::Allow,
            "{fragment}"
        );
    }

    // Legacy arrays remain ordered and replace public maps rather than inheriting their metadata.
    let legacy = r#"permissions: { rules: { shell: [
        { selector: { type: "exact", value: "git status" }, mode: "allow" },
        { selector: { type: "catch_all" }, mode: "deny" }
    ] } }"#;
    for (base, overlay, expected) in [
        (
            r#"permission: { bash: { "*": "allow" } }"#,
            legacy,
            PolicyDecision::Deny,
        ),
        (
            legacy,
            r#"permission: { bash: { "git status": "allow" } }"#,
            PolicyDecision::Allow,
        ),
        (
            r#"permission: { bash: { "*": "deny" } }"#,
            r#"permission: "allow""#,
            PolicyDecision::Allow,
        ),
        (
            r#"agent: { default: { permission: { bash: { "*": "allow", "git status": "deny" } } } }"#,
            r#"agent: { default: { model: "default/gpt-4o-mini" } }"#,
            PolicyDecision::Deny,
        ),
    ] {
        fs::write(&path, order_fixture(base)).unwrap_or_abort();
        let mut overlay_context = context.clone();
        overlay_context.runtime_content = Some(format!("{{ {overlay} }}"));
        let config = load_resolved_config_with_context(Some(&path), &overlay_context)
            .unwrap_or_else(|error| panic!("base={base} overlay={overlay}: {error}"))
            .unwrap_or_abort()
            .config;
        assert_eq!(
            PermissionPolicy::from_config(&config).evaluate_request(
                Some("default"),
                PermissionKind::Shell,
                Some(&PermissionRuleRequest::ShellCommand {
                    pattern: "git status".into()
                }),
            ),
            expected,
            "base={base} overlay={overlay}"
        );
    }
    assert!(
        load_config_from_str(&order_fixture(
            r#"permissions: { rules: { shell: { "*": "allow" } } }"#
        ))
        .is_err(),
        "internal rule arrays must not accept maps without source-order metadata"
    );
}
