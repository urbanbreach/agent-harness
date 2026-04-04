use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use harness_core::clock::RealClock;
use harness_core::config::{McpConfig, McpServerConfig, ShellAllowlist};
use harness_core::coord::{spawn_coordinator, CoordinatorConfig};
use harness_core::event::{ActorKind, EventActor};
use harness_core::redact::DefaultRedactor;
use harness_core::tool::ToolContext;
use harness_tools::coordinator_registry_with_mcp;
use serde_json::json;

fn test_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
    let coordinator = spawn_coordinator(
        CoordinatorConfig::default(),
        Arc::new(RealClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    ToolContext {
        run_id: "run-mcp-generic-tests".to_string(),
        workspace_root: workspace_root.to_path_buf(),
        artifacts_dir: workspace_root.join("artifacts"),
        actor: EventActor::new(ActorKind::Worker, Some("worker-1".to_string())),
        category: Some("deep".to_string()),
        plan_mode: false,
        plan_exit_target_profile: None,
        tool_call_id: tool_call_id.to_string(),
        coordinator,
    }
}

fn setup_workspace() -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    temp_dir
}

fn install_fake_mcp_server(script_path: &Path) {
    let script = r#"#!/usr/bin/env python3
import json
import sys

TOOLS = [{
    "name": "echo",
    "description": "Echoes text input",
    "inputSchema": {
        "type": "object",
        "properties": {
            "text": {"type": "string"}
        }
    }
}]
RESOURCES = [{
    "uri": "fixture://alpha",
    "name": "Alpha fixture"
}]
PROMPTS = [{
    "name": "summarize",
    "description": "Summarize a topic",
    "arguments": [{
        "name": "topic",
        "required": False
    }]
}]


def send(payload):
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    message = json.loads(raw)
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params", {})

    if method == "initialize" and message_id is not None:
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {
                "protocolVersion": "2025-06-18",
                "serverInfo": {"name": "fixture", "version": "1.0.0"},
                "capabilities": {
                    "tools": {"listChanged": False},
                    "resources": {"listChanged": False},
                    "prompts": {"listChanged": False}
                }
            }
        })
        continue

    if method == "notifications/initialized":
        continue

    if message_id is None:
        continue

    if method == "tools/list":
        send({"jsonrpc": "2.0", "id": message_id, "result": {"tools": TOOLS}})
    elif method == "tools/call":
        tool_name = params.get("name")
        arguments = params.get("arguments", {})
        if tool_name == "echo":
            send({
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "content": [{"type": "text", "text": arguments.get("text", "") }],
                    "isError": False
                }
            })
        else:
            send({
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "content": [{"type": "text", "text": f"unknown tool: {tool_name}"}],
                    "isError": True
                }
            })
    elif method == "resources/list":
        send({"jsonrpc": "2.0", "id": message_id, "result": {"resources": RESOURCES}})
    elif method == "resources/read":
        uri = params.get("uri", "fixture://missing")
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {
                "contents": [{
                    "uri": uri,
                    "mimeType": "text/plain",
                    "text": f"resource body for {uri}"
                }]
            }
        })
    elif method == "prompts/list":
        send({"jsonrpc": "2.0", "id": message_id, "result": {"prompts": PROMPTS}})
    elif method == "prompts/get":
        arguments = params.get("arguments", {})
        topic = arguments.get("topic", "unknown")
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {
                "messages": [{
                    "role": "user",
                    "content": [{"type": "text", "text": f"Summarize {topic}"}]
                }]
            }
        })
    else:
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "error": {
                "code": -32601,
                "message": f"unsupported method: {method}"
            }
        })
"#;
    fs::write(script_path, script).expect("write fake mcp server");
    let mut permissions = fs::metadata(script_path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions).expect("chmod fake mcp server");
}

fn fake_mcp_config(script_path: &Path) -> McpConfig {
    McpConfig {
        servers: BTreeMap::from([(
            "fixture".to_string(),
            McpServerConfig::Stdio {
                command: vec!["python3".to_string(), script_path.display().to_string()],
                env: BTreeMap::new(),
                cwd: None,
                timeout_secs: 5,
            },
        )]),
    }
}

#[tokio::test]
async fn generic_mcp_registry_exposes_server_scoped_tools() {
    let temp_dir = setup_workspace();
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));
    for tool_id in [
        "mcp.fixture.tools.list",
        "mcp.fixture.tool.call",
        "mcp.fixture.resources.list",
        "mcp.fixture.resource.read",
        "mcp.fixture.prompts.list",
        "mcp.fixture.prompt.get",
    ] {
        assert!(
            registry.get(tool_id).is_some(),
            "missing MCP tool {tool_id}"
        );
    }
}

#[tokio::test]
async fn generic_mcp_stdio_server_supports_tools_resources_and_prompts() {
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    let tools_list = registry
        .get("mcp.fixture.tools.list")
        .expect("tools.list tool");
    let tools_result = tools_list
        .call(test_context(&workspace, "mcp-tools-list"), json!({}))
        .await
        .expect("mcp tools.list");
    assert!(tools_result.display_text.contains("MCP tools from fixture"));
    assert!(tools_result
        .display_text
        .contains("echo — Echoes text input"));
    assert_eq!(
        tools_result
            .structured_json
            .as_ref()
            .and_then(|value| value.get("payload"))
            .and_then(|value| value.get("tools"))
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(1)
    );

    let tool_call = registry
        .get("mcp.fixture.tool.call")
        .expect("tool.call tool");
    let tool_result = tool_call
        .call(
            test_context(&workspace, "mcp-tool-call"),
            json!({
                "tool": "echo",
                "arguments": {
                    "text": "hello from mcp"
                }
            }),
        )
        .await
        .expect("mcp tool.call");
    assert_eq!(tool_result.display_text, "hello from mcp");
    assert_eq!(
        tool_result
            .structured_json
            .as_ref()
            .and_then(|value| value.get("payload"))
            .and_then(|value| value.get("result"))
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_array())
            .and_then(|entries| entries.first())
            .and_then(|entry| entry.get("text"))
            .and_then(|value| value.as_str()),
        Some("hello from mcp")
    );

    let resources_list = registry
        .get("mcp.fixture.resources.list")
        .expect("resources.list tool");
    let resources_result = resources_list
        .call(test_context(&workspace, "mcp-resources-list"), json!({}))
        .await
        .expect("mcp resources.list");
    assert!(resources_result.display_text.contains("fixture://alpha"));

    let resource_read = registry
        .get("mcp.fixture.resource.read")
        .expect("resource.read tool");
    let resource_result = resource_read
        .call(
            test_context(&workspace, "mcp-resource-read"),
            json!({ "uri": "fixture://alpha" }),
        )
        .await
        .expect("mcp resource.read");
    assert_eq!(
        resource_result.display_text,
        "resource body for fixture://alpha"
    );

    let prompts_list = registry
        .get("mcp.fixture.prompts.list")
        .expect("prompts.list tool");
    let prompts_result = prompts_list
        .call(test_context(&workspace, "mcp-prompts-list"), json!({}))
        .await
        .expect("mcp prompts.list");
    assert!(prompts_result
        .display_text
        .contains("summarize — Summarize a topic"));

    let prompt_get = registry
        .get("mcp.fixture.prompt.get")
        .expect("prompt.get tool");
    let prompt_result = prompt_get
        .call(
            test_context(&workspace, "mcp-prompt-get"),
            json!({
                "name": "summarize",
                "arguments": {
                    "topic": "MCP"
                }
            }),
        )
        .await
        .expect("mcp prompt.get");
    assert_eq!(prompt_result.display_text, "user: Summarize MCP");
}
