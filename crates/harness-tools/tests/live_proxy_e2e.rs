use harness_core::{
    clock::FakeClock,
    config::{McpConfig, McpServerConfig, ShellAllowlist},
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn public_exa_mcp_accepts_the_harness_transport() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("HARNESS_MCP_LIVE_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_MCP_LIVE_SIGNOFF=1 to contact the public Exa MCP server".into());
    }
    let root = tempfile::tempdir()?;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_mcp_tools(
        &mut registry,
        McpConfig {
            servers: [(
                "exa".into(),
                McpServerConfig::Http {
                    endpoint: "https://mcp.exa.ai/mcp?tools=web_search_exa,web_search_advanced_exa"
                        .into(),
                    headers: Default::default(),
                    timeout_secs: 10,
                    enabled: true,
                },
            )]
            .into(),
        },
    )?;
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::allow_all();
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("live-mcp", root.path()).await?;
    let result = handle
        .execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "mcp.exa.tools.list",
            json!({}),
        )
        .await;
    let result = result?;
    assert!(result.display_text.contains("web_search_exa"));
    assert!(result.display_text.contains("web_search_advanced_exa"));
    for (tool, args) in [
        (
            "websearch",
            json!({"query":"official Rust async book futures","numResults":2,"contextMaxCharacters":2000}),
        ),
        (
            "codesearch",
            json!({"query":"Tokio JoinSet spawn join_next documentation example","tokensNum":1000}),
        ),
    ] {
        let result = handle
            .execute_agent_tool_call(EventActor::new(ActorKind::User, None), None, tool, args)
            .await?;
        assert!(!result.is_error(), "{}", result.display_text);
        assert!(
            result.display_text.contains("https://"),
            "{}",
            result.display_text
        );
        println!(
            "{tool}: {}",
            result.structured_json.ok_or("search metadata missing")?
        );
    }
    handle.stop_run().await?;
    Ok(())
}
