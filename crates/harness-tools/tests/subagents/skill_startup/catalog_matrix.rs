use super::*;

#[tokio::test]
async fn native_skill_startup_selects_local_inherited_disabled_and_untrusted_catalogs(
) -> Result<(), Box<dyn std::error::Error>> {
    for preload in [false, true] {
        for (inherit, discover, trusted, deny_local, requested, expected_names, loads) in [
            (false, true, true, false, "local", vec!["local"], true),
            (false, false, true, false, "local", vec![], false),
            (
                true,
                false,
                true,
                false,
                "global",
                vec!["inherited", "global"],
                true,
            ),
            (true, true, false, false, "global", vec!["global"], true),
            (false, true, false, false, "local", vec![], false),
            (false, true, true, true, "local", vec!["local"], false),
        ] {
            let temp = tempfile::tempdir()?;
            let project = temp.path().join("project");
            let child_cwd = project.join("child");
            let global = temp.path().join("parent-global");
            fs::create_dir_all(&child_cwd)?;
            write_skill(
                &project.join("custom-skills/inherited"),
                "inherited",
                "INHERITED_BODY",
            )?;
            write_skill(&global.join("global"), "global", "GLOBAL_BODY")?;
            write_skill(
                &child_cwd.join(".harness/skills/local"),
                "local",
                "LOCAL_BODY",
            )?;
            let skills = SkillsConfig {
                project_roots: vec!["custom-skills".into()],
                global_roots: vec![global],
                walk_to_git_root: false,
                permissions: if deny_local {
                    std::collections::BTreeMap::from([("local".into(), PermissionMode::Deny)])
                } else {
                    Default::default()
                },
                ..Default::default()
            };
            let definition = SubagentDefinition {
                name: "skill-reader".into(),
                description: "Skill reader".into(),
                tools: vec!["Skill".into()],
                skills: if preload {
                    vec![requested.into()]
                } else {
                    vec![]
                },
                inherit_skills: inherit,
                discover_skills: discover,
                inject_default_tools: false,
                ..Default::default()
            };
            fs::create_dir_all(project.join(".harness/agents"))?;
            fs::write(
                project.join(".harness/agents/skill-reader.md"),
                format!("---\n{}---\n", serde_yaml_ng::to_string(&definition)?),
            )?;
            let provider = Arc::new(MockProvider::script([
                vec![
                    Stream::ToolCallComplete {
                        tool_call_id: "matrix-load".into(),
                        function_name: harness_providers::tool_function_name("skill"),
                        arguments_json: json!({"name":requested}).to_string(),
                    },
                    Stream::Done { usage: None },
                ],
                vec![
                    Stream::TextDelta("done".into()),
                    Stream::Done { usage: None },
                ],
            ]));
            let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
            config.skills = skills.clone();
            let discovery = Arc::new(CountedDiscovery::default());
            config.skill_catalog_discovery = Some(Arc::<CountedDiscovery>::clone(&discovery));
            config.subagent_discovery = Some(SubagentDiscoveryContext {
                cwd: project.clone(),
                project_trusted: trusted,
                user_root: None,
            });
            let registry =
                harness_tools::coordinator_registry_with_skills(ShellAllowlist::default(), skills);
            let mut parent = AgentProfile::fallback("default");
            parent.toolset = registry.tool_ids();
            config.agent_profiles.insert("default".into(), parent);
            config.tool_registry = Arc::new(registry);
            config.permission_policy = if deny_local {
                PermissionPolicy::from_rules(vec![
                    harness_core::perm::PermissionRule {
                        permission: "*".into(),
                        pattern: "*".into(),
                        action: PermissionMode::Allow,
                    },
                    harness_core::perm::PermissionRule {
                        permission: "skill".into(),
                        pattern: "local".into(),
                        action: PermissionMode::Deny,
                    },
                ])?
            } else {
                PermissionPolicy::allow_all()
            };
            config.provider = Arc::<MockProvider>::clone(&provider);
            let coordinator = spawn_coordinator(
                config,
                Arc::new(FakeClock::new()),
                Arc::new(DefaultRedactor::default()),
            );
            coordinator
                .start_run("skill catalog matrix", &project)
                .await?;
            let parent = coordinator
                .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
                .await?;
            tokio::time::timeout(
            Duration::from_secs(5),
            coordinator.execute_agent_tool_call(
                EventActor::new(ActorKind::Worker, Some(parent)),
                None, "spawn_subagent",
                json!({"prompt":"load selected skill","description":"Matrix skill child","subagent_type":"skill-reader","background":false,"cwd":child_cwd}),
            ),
        ).await??;
            let requests = provider.captured_requests().await;
            coordinator.stop_run().await?;
            let initial = requests.first().ok_or("initial matrix request")?;
            let metadata = available_skills(initial)?;
            assert_eq!(
                discovery.0.load(Ordering::SeqCst),
                usize::from(inherit || discover),
                "disabled discovery ran a filesystem catalog scan",
            );
            // Ignore unrelated default user skills; custom parent roots must never
            // survive the false-inheritance discovery reset.
            for name in ["inherited", "global", "local"] {
                assert_eq!(
                metadata.iter().any(|entry| entry["name"] == name),
                expected_names.contains(&name) && !(preload && loads && name == requested),
                "inherit={inherit} discover={discover} trusted={trusted} deny={deny_local}: {name}",
            );
            }
            assert!(metadata.iter().all(|entry| entry["body_loaded"] == false));
            if deny_local {
                let local = metadata
                    .iter()
                    .find(|entry| entry["name"] == "local")
                    .ok_or("denied local")?;
                assert_eq!(local["permission_mode"], "deny");
                assert_eq!(local["status"], "denied");
                assert_eq!(local["loadable"], false);
            }
            assert!(initial
                .tools
                .as_ref()
                .is_some_and(|tools| tools.iter().any(|tool| tool.tool_id == "skill")));
            let initial_json = serde_json::to_string(initial)?;
            for sentinel in ["INHERITED_BODY", "GLOBAL_BODY", "LOCAL_BODY"] {
                assert_eq!(
                    initial_json.contains(sentinel),
                    preload
                        && loads
                        && sentinel
                            == if requested == "global" {
                                "GLOBAL_BODY"
                            } else {
                                "LOCAL_BODY"
                            }
                );
            }
            let after = requests.get(1).ok_or("post-tool matrix request")?;
            let results = after
                .messages
                .iter()
                .filter(|message| message.role == harness_providers::MessageRole::Tool)
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let sentinel = if requested == "global" {
                "GLOBAL_BODY"
            } else {
                "LOCAL_BODY"
            };
            assert_eq!(results.contains(sentinel), loads);
            if deny_local {
                assert!(results.contains("permission denied"));
            }
        }
    }
    Ok(())
}
