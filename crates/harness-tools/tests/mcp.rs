use harness_core::{
    agent::AgentProfile,
    clock::FakeClock,
    config::{McpConfig, McpServerConfig, ShellAllowlist},
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    sync::{Mutex, Notify},
};
use tokio_stream::StreamExt;
#[path = "mcp/eval.rs"]
mod eval;
#[path = "mcp/server.rs"]
mod server;
use server::serve;

const IMAGE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

#[tokio::test]
async fn mcp_connects_lazily_reuses_sessions_and_reads_paginated_content(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = format!("http://{}/mcp", listener.local_addr()?);
    let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
    let seen = Arc::clone(&requests);
    let changed = Arc::new(Notify::new());
    let notify = Arc::clone(&changed);
    let server = tokio::spawn(serve(listener, seen, notify));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "mcp.fixture.tool.call".into(),
            pattern: "forbidden".into(),
            action: PermissionAction::Deny,
        },
        PermissionRule {
            permission: "mcp.fixture.secret".into(),
            pattern: "*".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_mcp_tools(
        &mut registry,
        McpConfig {
            servers: [(
                "fixture".into(),
                McpServerConfig::Http {
                    endpoint: endpoint.clone(),
                    headers: [("authorization".into(), "Bearer opaque-mcp-fixture".into())].into(),
                    timeout_secs: 3,
                    enabled: true,
                },
            )]
            .into(),
        },
    )?;
    let marker = root.path().join("stdio-started");
    harness_tools::register_mcp_tools(
        &mut registry,
        McpConfig {
            servers: [(
                "process".into(),
                McpServerConfig::Stdio {
                    command: vec!["touch".into(), marker.to_string_lossy().into_owned()],
                    env: Default::default(),
                    cwd: None,
                    timeout_secs: 1,
                    enabled: true,
                },
            )]
            .into(),
        },
    )?;
    config.tool_registry = Arc::new(registry);
    let mut denied_config = config.clone();
    denied_config.permission_policy = PermissionPolicy::from_rules(
        [
            ("*", PermissionAction::Allow),
            ("network", PermissionAction::Deny),
            ("bash", PermissionAction::Deny),
        ]
        .into_iter()
        .map(|(permission, action)| PermissionRule {
            permission: permission.into(),
            pattern: "*".into(),
            action,
        })
        .collect(),
    )?;
    let denied = spawn_coordinator(
        denied_config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    denied.start_run("transport denied", root.path()).await?;
    for tool in ["mcp.fixture.tools.list", "mcp.process.tools.list"] {
        let result = denied
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                tool,
                json!({}),
            )
            .await;
        assert!(
            matches!(&result, Err(error) if error.contains("permission denied")),
            "{result:?}"
        );
    }
    assert!(
        requests.lock().await.is_empty(),
        "transport denial must precede initialization"
    );
    assert!(
        !marker.exists(),
        "bash denial must prevent spawning the MCP process"
    );
    let provider = Arc::new(MockProvider::script([
        vec![
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "discover".into(),
                function_name: harness_providers::tool_function_name("mcp.fixture.tools.list"),
                arguments_json: "{}".into(),
            },
            ProviderStreamEvent::Done { usage: None },
        ],
        vec![
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "echo".into(),
                function_name: harness_providers::tool_function_name("mcp.fixture.echo"),
                arguments_json: "{}".into(),
            },
            ProviderStreamEvent::Done { usage: None },
        ],
        vec![
            ProviderStreamEvent::TextDelta("done".into()),
            ProviderStreamEvent::Done { usage: None },
        ],
    ]));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    let mut profile = AgentProfile::fallback("default");
    profile.toolset = vec![
        "mcp.fixture.tools.list".into(),
        "mcp.fixture.tool.call".into(),
        "mcp.fixture.zz_129".into(),
    ];
    config.agent_profiles.insert("default".into(), profile);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = coordinator.start_run("mcp", root.path()).await?;
    for args in [
        json!({"tool":"secret"}),
        json!({"tool":"forbidden"}),
        json!({"tool":"echo","arguments":false}),
        json!({"tool":"echo","extra":true}),
    ] {
        assert!(coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "mcp.fixture.tool.call",
                args
            )
            .await
            .is_err());
    }
    assert!(
        requests.lock().await.is_empty(),
        "registry construction must stay offline: {endpoint}"
    );
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    let mut events = coordinator.subscribe_new_events().await?;
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent,
            "Use the MCP tool.",
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload, EventV1::TaskCompleted(ref t) if t.task_id.as_str() == turn)
            {
                return Ok::<_, Box<dyn std::error::Error>>(());
            }
        }
        Err("turn did not finish".into())
    })
    .await??;
    let provider_requests = provider.captured_requests().await;
    let before = requests.lock().await.len();
    let result = denied
        .execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "mcp.fixture.echo",
            json!({}),
        )
        .await;
    assert!(
        matches!(&result, Err(error) if error.contains("permission denied")),
        "{result:?}"
    );
    assert_eq!(
        requests.lock().await.len(),
        before,
        "discovered tools must retain the transport gate"
    );
    denied.stop_run().await?;
    let tools = provider_requests[1].tools.as_ref().ok_or("missing tools")?;
    assert!(
        tools.len() <= 128,
        "discovery must not overflow the provider tool limit"
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool.tool_id == "mcp.fixture.zz_129"),
        "explicit tools take priority over catalog expansion"
    );
    assert!(tools
        .iter()
        .any(|tool| tool.tool_id == "mcp.fixture.tool.call"));
    for protocol in [
        harness_providers::Protocol::Chat,
        harness_providers::Protocol::Responses,
        harness_providers::Protocol::Anthropic,
    ] {
        let wire =
            harness_providers::HttpProvider::new(&endpoint, protocol, Duration::from_secs(3))?;
        for request in &provider_requests {
            harness_providers::Provider::request_budget_semantics(&wire, request, 0)?;
        }
    }
    assert!(
        provider_requests[1]
            .tools
            .as_ref()
            .ok_or("tools missing")?
            .iter()
            .any(|tool| tool.tool_id == "mcp.fixture.echo"),
        "discovered schemas must be available within the same turn"
    );
    for (id, args, expected) in [
        ("tools.list", json!({}), "other"),
        ("echo", json!({"text":"direct call"}), "answer"),
        (
            "tool.call",
            json!({"tool":"echo","arguments":{"text":"hello"}}),
            "answer",
        ),
        ("tool.call", json!({"tool":"sse","arguments":{}}), "answer"),
        ("tool.call", json!({"tool":"zz_128"}), "answer"),
        ("tool.call", json!({"tool":"structured"}), "structured-only"),
        ("resources.list", json!({}), "fixture://notes"),
        (
            "resource.read",
            json!({"uri":"fixture://notes"}),
            "resource body",
        ),
        ("prompts.list", json!({}), "review"),
        (
            "prompt.get",
            json!({"name":"review","arguments":{}}),
            "Review this change.",
        ),
    ] {
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            coordinator.execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                format!("mcp.fixture.{id}"),
                args,
            ),
        )
        .await??;
        assert!(
            result.display_text.contains(expected),
            "{}",
            result.display_text
        );
    }
    for (method, args) in [
        ("tool.call", json!({"tool":"image"})),
        ("resource.read", json!({"uri":"fixture://image"})),
        ("prompt.get", json!({"name":"image"})),
    ] {
        let image = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                format!("mcp.fixture.{method}"),
                args,
            )
            .await?;
        assert_eq!(image.attachments.len(), 1);
        assert_eq!(image.attachments[0].mime, "image/png");
        assert!(!image.display_text.contains(IMAGE));
        assert!(!serde_json::to_string(&image)?.contains(IMAGE));
    }
    coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "mcp.fixture.tool.call",
            json!({"tool":"refresh-sse"}),
        )
        .await?;
    assert!(
        coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "mcp.fixture.echo",
                json!({})
            )
            .await
            .is_err(),
        "a server list-change notification must invalidate old schemas"
    );
    coordinator
        .execute_agent_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "mcp.fixture.tools.list",
            json!({}),
        )
        .await?;
    let pending = coordinator
        .request_tool_call(
            EventActor::new(ActorKind::User, None),
            None,
            "mcp.fixture.tool.call",
            json!({"tool":"slow"}),
        )
        .await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if requests
                .lock()
                .await
                .iter()
                .any(|r| r["method"] == "tools/call" && r["params"]["name"] == "slow")
            {
                break;
            }
            changed.notified().await;
        }
    })
    .await?;
    coordinator.cancel_task(pending, "stop waiting").await?;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if requests
                .lock()
                .await
                .iter()
                .any(|r| r["method"] == "notifications/cancelled")
            {
                break;
            }
            changed.notified().await;
        }
    })
    .await?;
    for tool in ["large-json", "large-sse"] {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            coordinator.execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "mcp.fixture.tool.call",
                json!({"tool":tool}),
            ),
        )
        .await?;
        assert!(result.is_err(), "oversized MCP responses must fail");
    }
    assert_eq!(
        harness_core::config::registered_mcp_server_connection_state("fixture"),
        Some(harness_core::config::McpServerConnectionState::Connected)
    );
    assert_eq!(
        harness_core::config::registered_mcp_server_first_class_tool_id("fixture", "echo")
            .as_deref(),
        Some("mcp.fixture.echo")
    );
    coordinator.stop_run().await?;
    assert!(harness_core::config::registered_mcp_server_connection_state("fixture").is_none());
    let journal = std::fs::read_to_string(info.events_path)?;
    assert!(!journal.contains(IMAGE));
    assert!(
        !journal.contains("opaque-mcp-fixture"),
        "configured credentials must never enter the journal"
    );
    assert_eq!(
        requests
            .lock()
            .await
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    server.abort();
    Ok(())
}
