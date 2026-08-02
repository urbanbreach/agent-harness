use harness_tools::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::Path;

use harness_core::config::{McpConfig, McpServerConfig, ShellAllowlist};
use harness_tools::coordinator_registry_with_mcp;
use serde_json::json;

mod common;

use common::{
    install_fake_mcp_server, install_fake_mcp_server_with_tools,
    install_stateful_terminal_mcp_server, setup_workspace, test_context as common_test_context,
};

fn test_context(workspace_root: &Path, tool_call_id: &str) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-mcp-generic-tests", tool_call_id)
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
                enabled: true,
            },
        )]),
    }
}

fn assert_wrapped_tool_result_contract(
    result: &harness_core::tool::ToolResult,
    expected_tool: &str,
    expected_text: &str,
) {
    let structured = result.structured_json.as_ref().unwrap_or_abort();
    assert_eq!(
        structured
            .get("server")
            .and_then(|value| value.get("id"))
            .and_then(|value| value.as_str()),
        Some("fixture")
    );
    assert_eq!(
        structured
            .get("protocolVersion")
            .and_then(|value| value.as_str()),
        Some("2025-06-18")
    );
    assert_eq!(
        structured
            .get("serverInfo")
            .and_then(|value| value.get("name"))
            .and_then(|value| value.as_str()),
        Some("fixture")
    );
    assert_eq!(
        structured
            .get("payload")
            .and_then(|value| value.get("tool"))
            .and_then(|value| value.as_str()),
        Some(expected_tool)
    );
    assert_eq!(
        structured
            .get("payload")
            .and_then(|value| value.get("result"))
            .and_then(|value| value.get("content"))
            .and_then(|value| value.as_array())
            .and_then(|entries| entries.first())
            .and_then(|entry| entry.get("text"))
            .and_then(|value| value.as_str()),
        Some(expected_text)
    );
}

#[tokio::test]
async fn generic_mcp_registry_exposes_server_scoped_tools() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));
    for tool_id in [
        "mcp.fixture.echo",
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
async fn stdio_mcp_tool_calls_preserve_stateful_sessions_across_calls() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("stateful_terminal_mcp_server.py");
    install_stateful_terminal_mcp_server(&script_path);

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));
    let spawn = registry.get("mcp.fixture.terminal_spawn").unwrap_or_abort();
    let wait = registry.get("mcp.fixture.terminal_wait").unwrap_or_abort();

    let spawn_result = spawn
        .call(
            test_context(&workspace, "pty-spawn"),
            json!({"shell": "/bin/bash"}),
        )
        .await
        .unwrap_or_abort();
    assert!(spawn_result.display_text.contains("sessionId"));

    let wait_result = wait
        .call(
            test_context(&workspace, "pty-wait"),
            json!({"sessionId": "term-1", "ms": 10}),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(wait_result.display_text, "terminal session still active");
}

#[tokio::test]
async fn generic_mcp_registry_exposes_first_class_remote_tools() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    let tool = registry.get("mcp.fixture.echo").unwrap_or_abort();
    let result = tool
        .call(
            test_context(&workspace, "mcp-first-class-tool-call"),
            json!({ "text": "hello first-class" }),
        )
        .await
        .unwrap_or_abort();

    assert_eq!(result.display_text, "hello first-class");
    assert_wrapped_tool_result_contract(&result, "echo", "hello first-class");
}

#[tokio::test]
async fn generic_mcp_stdio_server_supports_tools_resources_and_prompts() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    let tools_list = registry.get("mcp.fixture.tools.list").unwrap_or_abort();
    let tools_result = tools_list
        .call(test_context(&workspace, "mcp-tools-list"), json!({}))
        .await
        .unwrap_or_abort();
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

    let tool_call = registry.get("mcp.fixture.tool.call").unwrap_or_abort();
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
        .unwrap_or_abort();
    assert_eq!(tool_result.display_text, "hello from mcp");
    assert_wrapped_tool_result_contract(&tool_result, "echo", "hello from mcp");

    let resources_list = registry.get("mcp.fixture.resources.list").unwrap_or_abort();
    let resources_result = resources_list
        .call(test_context(&workspace, "mcp-resources-list"), json!({}))
        .await
        .unwrap_or_abort();
    assert!(resources_result.display_text.contains("fixture://alpha"));

    let resource_read = registry.get("mcp.fixture.resource.read").unwrap_or_abort();
    let resource_result = resource_read
        .call(
            test_context(&workspace, "mcp-resource-read"),
            json!({ "uri": "fixture://alpha" }),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(
        resource_result.display_text,
        "resource body for fixture://alpha"
    );

    let prompts_list = registry.get("mcp.fixture.prompts.list").unwrap_or_abort();
    let prompts_result = prompts_list
        .call(test_context(&workspace, "mcp-prompts-list"), json!({}))
        .await
        .unwrap_or_abort();
    assert!(prompts_result
        .display_text
        .contains("summarize — Summarize a topic"));

    let prompt_get = registry.get("mcp.fixture.prompt.get").unwrap_or_abort();
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
        .unwrap_or_abort();
    assert_eq!(prompt_result.display_text, "user: Summarize MCP");
}

#[tokio::test]
async fn generic_mcp_registry_reserves_wrapper_ids_for_colliding_first_class_tools() {
    // arrange
    // act
    // assert
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server_with_tools(
        &script_path,
        r#"[
    {
        "name": "tool_call_2",
        "description": "Looks like a disambiguated wrapper collision",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            }
        }
    },
    {
        "name": "tool.call",
        "description": "Collides with the reserved tool.call wrapper id",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            }
        }
    },
    {
        "name": "tool_call",
        "description": "Shares the same sanitized segment as tool.call",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            }
        }
    },
    {
        "name": "tools.list",
        "description": "Collides with the reserved tools.list wrapper id",
        "inputSchema": {
            "type": "object",
            "properties": {
                "text": {"type": "string"}
            }
        }
    }
]"#,
    );

    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    for tool_id in [
        "mcp.fixture.tool.call",
        "mcp.fixture.tools.list",
        "mcp.fixture.tool_call_2",
        "mcp.fixture.tool_call_3",
        "mcp.fixture.tool_call_2_2",
        "mcp.fixture.tools_list_2",
    ] {
        assert!(
            registry.get(tool_id).is_some(),
            "missing reserved/disambiguated MCP tool {tool_id}"
        );
    }

    let reserved_tool = registry.get("mcp.fixture.tool_call_2").unwrap_or_abort();
    let reserved_result = reserved_tool
        .call(
            test_context(&workspace, "mcp-reserved-tool-call-direct"),
            json!({ "text": "reserved tool.call" }),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(reserved_result.display_text, "reserved tool.call");
    assert_wrapped_tool_result_contract(&reserved_result, "tool.call", "reserved tool.call");

    let wrapper_tool = registry.get("mcp.fixture.tool.call").unwrap_or_abort();
    let wrapper_result = wrapper_tool
        .call(
            test_context(&workspace, "mcp-reserved-tool-call-wrapper"),
            json!({
                "tool": "tool.call",
                "arguments": { "text": "wrapper reserved tool.call" },
            }),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(wrapper_result.display_text, "wrapper reserved tool.call");
    assert_wrapped_tool_result_contract(&wrapper_result, "tool.call", "wrapper reserved tool.call");
}
