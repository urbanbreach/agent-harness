use super::*;

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn eval_mcp_discovery_updates_the_same_cells_schema_and_tool_namespace(
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_EVAL_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_EVAL_SIGNOFF=1 to run actual eval runtimes".into());
    }
    let root = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/mcp", listener.local_addr()?);
    let server = tokio::spawn(serve(
        listener,
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Notify::new()),
    ));
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_eval_tool(
        &mut registry,
        harness_core::config::EvalConfig {
            route_tools: vec![
                "mcp.fixture.tools.list".into(),
                "mcp.fixture.tool.call".into(),
            ],
            ..Default::default()
        },
    );
    harness_tools::register_mcp_tools(
        &mut registry,
        McpConfig {
            servers: [(
                "fixture".into(),
                McpServerConfig::Http {
                    endpoint,
                    headers: Default::default(),
                    timeout_secs: 3,
                    enabled: true,
                },
            )]
            .into(),
        },
    )?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = registry.tool_ids();
    config.agent_profiles.insert("default".into(), profile);
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    config.secret_values = vec!["opaque-mcp-fixture".into()];
    let provider = Arc::new(MockProvider::script([
        vec![
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "routed-mcp".into(),
                function_name: "eval".into(),
                arguments_json: json!({"language":"js","summary":"Read a discovered tool through eval","code":"display(await tool['mcp.fixture.echo']({value:42}))"}).to_string(),
            },
            ProviderStreamEvent::Done { usage: None },
        ],
        vec![ProviderStreamEvent::TextDelta("done".into()), ProviderStreamEvent::Done { usage: None }],
    ]));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = coordinator.start_run("eval-mcp", root.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    for (language, code) in [
        ("js", "await tool['mcp.fixture.tools.list']({}); var collision; try { tool(function mcp_fixture_echo() {}); } catch(error) { collision = error.code; } if(collision !== 'tool_name_collision') throw new Error('discovered host tool was shadowed'); display(await tool_schema('mcp.fixture.echo')); display(await tool['mcp.fixture.echo']({value:42}));"),
        ("py", "getattr(tool,'mcp.fixture.tools.list')()\ndisplay(tool_schema('mcp.fixture.echo'))\ndisplay(getattr(tool,'mcp.fixture.echo')(value=42))"),
    ] {
        let result = tokio::time::timeout(Duration::from_secs(15), coordinator.execute_agent_tool_call(
            EventActor::new(ActorKind::Worker, Some(agent.clone())), None, "eval",
            json!({"language":language,"code":code,"summary":"Discover and invoke an MCP tool","on_timeout":"error"}),
        )).await??;
        assert!(!result.is_error(), "{}", result.display_text);
        assert!(result.display_text.contains("parameters") && result.display_text.contains("answer [REDACTED]"));
    }
    let mut events = coordinator.subscribe_new_events().await?;
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent,
            "Read the discovered tool",
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(event) if event.task_id.as_str() == turn => {
                    return Ok::<_, Box<dyn std::error::Error>>(())
                }
                EventV1::TaskCancelled(event) if event.task_id.as_str() == turn => {
                    return Err("routed MCP turn failed".into())
                }
                _ => {}
            }
        }
        Err("MCP turn did not settle".into())
    })
    .await??;
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 2);
    for request in &requests {
        let tools = request.tools.as_ref().ok_or("missing tool definitions")?;
        assert!(tools.iter().any(|tool| tool.tool_id == "eval"));
        assert!(!tools.iter().any(|tool| matches!(
            tool.tool_id.as_str(),
            "mcp.fixture.echo" | "mcp.fixture.tools.list" | "mcp.fixture.tool.call"
        )));
    }
    assert!(requests[1]
        .messages
        .iter()
        .any(|message| message.content.contains("answer [REDACTED]")));
    coordinator.stop_run().await?;
    server.abort();
    let _ = server.await;
    assert!(!std::fs::read_to_string(&info.events_path)?.contains("opaque-mcp-fixture"));
    Ok(())
}
