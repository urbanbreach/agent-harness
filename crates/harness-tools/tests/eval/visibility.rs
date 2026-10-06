use super::*;
use harness_providers::{mock::MockProvider, Provider, ProviderStreamEvent as Stream};

#[tokio::test]
#[ignore = "requires Node.js 24+; scripts/test-lanes.sh eval"]
async fn eval_routing_hides_schemas_without_changing_permissions_or_direct_fallback(
) -> Result<(), Box<dyn std::error::Error>> {
    signoff()?;
    for (route, enabled, permitted, isolate) in [
        (true, true, true, false),
        (true, true, true, true),
        (false, true, true, false),
        (true, false, true, false),
        (true, true, false, false),
    ] {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("allowed.txt"), "visible read result")?;
        std::fs::write(root.path().join("blocked.txt"), "must not be read")?;
        let routed = route && enabled && permitted;
        let arguments = if routed {
            json!({"language":"js","isolate":isolate,"summary":"Read through the permitted tool bridge","code":
                "display(await tool_schema('read')); display(await tool.read({path:'allowed.txt'})); try { await tool.read({path:'blocked.txt'}); throw new Error('denial bypassed'); } catch (error) { if (!String(error).includes('denied')) throw error; print('denial respected'); }"})
        } else {
            json!({"path":"allowed.txt"})
        };
        let provider = Arc::new(MockProvider::script([
            vec![
                Stream::ToolCallComplete {
                    tool_call_id: "read-call".into(),
                    function_name: if routed { "eval" } else { "read" }.into(),
                    arguments_json: arguments.to_string(),
                },
                Stream::Done { usage: None },
            ],
            vec![
                Stream::TextDelta("done".into()),
                Stream::Done { usage: None },
            ],
        ]));
        let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
        let mut eval_config = harness_core::config::EvalConfig {
            route_tools: if route {
                vec!["read".into()]
            } else {
                Vec::new()
            },
            ..Default::default()
        };
        eval_config.sandbox.enabled = isolate;
        harness_tools::register_eval_tool(&mut registry, eval_config);
        let mut config = CoordinatorConfig::new(root.path().join("sessions"));
        config.tool_registry = Arc::new(registry);
        config.provider = Arc::clone(&provider) as Arc<dyn Provider>;
        config.agent_prompt_sources.insert(
            "default".into(),
            Arc::new(harness_core::system_prompt::PromptSource::default()),
        );
        config.permission_policy = PermissionPolicy::from_rules(vec![
            PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "read".into(),
                pattern: "*blocked.txt".into(),
                action: PermissionAction::Deny,
            },
            PermissionRule {
                permission: "eval".into(),
                pattern: "*".into(),
                action: if permitted {
                    PermissionAction::Allow
                } else {
                    PermissionAction::Deny
                },
            },
        ])?;
        let mut profile = AgentProfile::fallback("default");
        profile.model_ref = "mock:eval".into();
        profile.toolset = vec!["read".into()];
        if enabled {
            profile.toolset.push("eval".into());
        }
        config.agent_profiles.insert("default".into(), profile);
        config
            .agent_model_targets
            .insert("default".into(), model_target());
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("eval routing", root.path()).await?;
        let agent = coordinator
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        let mut events = coordinator.subscribe_new_events().await?;
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent,
                "Read allowed.txt",
            )
            .await?;
        tokio::time::timeout(Duration::from_secs(20), finished(&mut events, &turn)).await??;
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let system = &request.messages[0].content;
            assert_eq!(system.contains("eval: tool.read"), routed);
            assert_eq!(system.contains("# Eval"), enabled && permitted);
            let tools = request.tools.as_ref().ok_or("missing tool definitions")?;
            assert_eq!(tools.iter().any(|tool| tool.tool_id == "read"), !routed);
            assert_eq!(
                tools.iter().any(|tool| tool.tool_id == "eval"),
                enabled && permitted
            );
        }
        let messages = &requests[1].messages;
        assert!(messages
            .iter()
            .any(|message| message.content.contains("visible read result")));
        assert!(!messages
            .iter()
            .any(|message| message.content.contains("must not be read")));
        if routed {
            assert!(messages
                .iter()
                .any(|message| message.content.contains("denial respected")));
        }
        coordinator.stop_run().await?;
    }
    Ok(())
}

async fn finished(
    events: &mut harness_core::store::EventStream,
    turn: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    while let Some(event) = events.next().await {
        match event?.payload {
            EventV1::TaskCompleted(event) if event.task_id.as_str() == turn => return Ok(()),
            EventV1::TaskCancelled(event) if event.task_id.as_str() == turn => {
                return Err("routed turn failed".into())
            }
            _ => {}
        }
    }
    Err("turn did not settle".into())
}
