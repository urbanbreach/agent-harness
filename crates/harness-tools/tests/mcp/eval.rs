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
    coordinator.stop_run().await?;
    server.abort();
    let _ = server.await;
    assert!(!std::fs::read_to_string(&info.events_path)?.contains("opaque-mcp-fixture"));
    Ok(())
}
