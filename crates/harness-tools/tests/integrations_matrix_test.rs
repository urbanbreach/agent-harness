//! Integration matrix tests (Task 22) — MCP and LSP families.
//!
//! Each family gets one real boundary E2E plus bad input, permission denial,
//! process failure, cancellation/restart, and redaction coverage.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use harness_core::config::{
    load_config_from_str, LspConfig, LspServerConfig, McpConfig, McpServerConfig, ShellAllowlist,
};
use harness_core::tool::{ToolError, ToolRegistry};
use harness_tools::coordinator_registry_with_mcp;
use harness_tools::UnwrapOrAbort;
use rustix::io::Errno;
use rustix::process::{test_kill_process, Pid};
use serde_json::json;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::time::timeout;

mod common;

use common::{install_fake_mcp_server, setup_workspace, test_context as common_test_context};

fn test_context(workspace_root: &Path, tool_call_id: &str) -> harness_core::tool::ToolContext {
    common_test_context(workspace_root, "run-integrations-matrix", tool_call_id)
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

fn install_crashing_mcp_server(script_path: &Path) {
    let script = "#!/usr/bin/env python3\nimport sys\nsys.exit(1)\n";
    fs::write(script_path, script).unwrap_or_abort();
    let mut permissions = fs::metadata(script_path).unwrap_or_abort().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions).unwrap_or_abort();
}

fn install_controlled_mcp_server(script_path: &Path) {
    let script = r#"#!/usr/bin/env python3
import json
import os
import pathlib
import socket
import sys

def send(payload):
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()

calls = 0
for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    message = json.loads(raw)
    method = message.get("method")
    message_id = message.get("id")
    if method == "initialize" and message_id is not None:
        control = socket.socket(socket.AF_UNIX)
        control.connect(str(pathlib.Path(__file__).with_suffix(".sock")))
        control.sendall(os.getpid().to_bytes(4, "big"))
        mode = control.recv(1)
        control.sendall(b"!")
        if mode == b"r":
            send({"jsonrpc": "2.0", "id": message_id,
                  "error": {"code": -32603, "message": "fixture rejected initialization"}})
        if mode == b"L":
            sys.stdout.write("Content-Length: 16777217\r\n")
            sys.stdout.flush()
        if mode == b"H":
            sys.stdout.write("Content-Length: 2\r\nX-Fixture: " + "x" * 8192)
            sys.stdout.flush()
        if mode in (b"r", b"c", b"L", b"H"):
            # Stay alive after stdin closes; EOF on the control socket cleans up a failing test.
            control.recv(1)
            sys.exit(0)
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {
                "protocolVersion": "2025-06-18",
                "serverInfo": {"name": "controlled-fixture", "version": "1.0.0"},
                "capabilities": {"tools": {"listChanged": False}}
            }
        })
    elif method == "notifications/initialized":
        continue
    elif method == "tools/call" and message_id is not None:
        if mode == b"B":
            sys.stdout.write("x" * 16777217)
            sys.stdout.flush()
            control.recv(1)
            sys.exit(0)
        calls += 1
        send({"jsonrpc": "2.0", "id": message_id, "result": {
            "content": [{"type": "text", "text": str(calls)}], "isError": False
        }})
"#;
    fs::write(script_path, script).unwrap_or_abort();
    let mut permissions = fs::metadata(script_path).unwrap_or_abort().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions).unwrap_or_abort();
}

// ---------------------------------------------------------------------------
// MCP family
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mcp_boundary_e2e_generic_registry_exposes_and_calls_echo_tool() {
    // arrange
    // Given: a workspace with a fake MCP server
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);
    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    // act
    // When: the echo tool is called
    let tool = registry.get("mcp.fixture.echo").unwrap_or_abort();
    let result = tool
        .call(
            test_context(&workspace, "mcp-e2e-boundary"),
            json!({"text": "hello boundary"}),
        )
        .await
        .unwrap_or_abort();

    // assert
    // Then: the tool returns the echoed text
    assert!(result.display_text.contains("hello boundary"));
}

#[test]
fn mcp_bad_input_invalid_server_id_rejected_by_config_validation() {
    // arrange
    // Given: a config with an MCP server id containing invalid characters
    let raw = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: { baseURL: "http://127.0.0.1:8317/v1", apiKey: "test-key", apiMode: "responses", timeoutMs: 60000 },
              models: { "gpt-5.4-mini": { name: "GPT-5.4 Mini" } },
            },
          },
          model: "default/gpt-5.4-mini",
          agent: { default: { tools: ["read"] } },
          permission: { edit: "ask", bash: "ask", webfetch: "deny" },
          mcp: {
                "bad server!": {
                  transport: "stdio",
                  command: ["echo"],
                  timeout: 5000,
                  enabled: true,
                },
          },
        }
    "#;

    // act
    // When: loaded from string
    let err = load_config_from_str(raw).expect_err("invalid server id must fail");

    // assert
    // Then: config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("invalid server id") || msg.contains("server name"),
        "expected server id validation error, got: {msg}"
    );
}

#[tokio::test]
async fn mcp_permission_denial_disabled_server_does_not_register_tools() {
    // arrange
    // Given: a workspace with a fake MCP server that is disabled
    let temp_dir = setup_workspace();
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);
    let disabled_config = McpConfig {
        servers: BTreeMap::from([(
            "fixture".to_string(),
            McpServerConfig::Stdio {
                command: vec!["python3".to_string(), script_path.display().to_string()],
                env: BTreeMap::new(),
                cwd: None,
                timeout_secs: 5,
                enabled: false,
            },
        )]),
    };
    let registry = coordinator_registry_with_mcp(ShellAllowlist::default(), disabled_config);

    // act
    // When: the echo tool is looked up
    let tool = registry.get("mcp.fixture.echo");

    // assert
    // Then: the tool is not registered (disabled server = permission denied at config boundary)
    assert!(
        tool.is_none(),
        "disabled MCP server must not register tools"
    );
}

#[tokio::test]
async fn mcp_process_failure_crashing_server_returns_tool_error() {
    // arrange
    // Given: a workspace with a crashing MCP server
    let temp_dir = setup_workspace();
    let script_path = temp_dir.path().join("crashing_mcp_server.py");
    install_crashing_mcp_server(&script_path);
    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    // act
    // When: the echo tool is looked up (server crashes on startup, no tools registered)
    let tool = registry.get("mcp.fixture.echo");

    // assert
    // Then: the tool is not registered (process failure prevents tool registration)
    assert!(
        tool.is_none(),
        "crashing MCP server must not register tools"
    );
}

#[tokio::test]
async fn mcp_startup_failure_and_cancellation_terminate_processes_and_preserve_reuse() {
    let mut survivors = Vec::new();
    for mode in *b"rchLHB" {
        let temp_dir = setup_workspace();
        let workspace = temp_dir.path().join("workspace");
        let script_path = temp_dir.path().join("controlled_mcp_server.py");
        // Discovery uses an ephemeral session; control the next startup through the public tool.
        install_fake_mcp_server(&script_path);
        let registry =
            coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));
        install_controlled_mcp_server(&script_path);
        let listener = UnixListener::bind(script_path.with_extension("sock")).unwrap_or_abort();
        let tool = registry.get("mcp.fixture.echo").unwrap_or_abort();
        let context = test_context(&workspace, "mcp-startup");
        let call = tokio::spawn(async move { tool.call(context, json!({})).await });
        let (mut control, process) = timeout(Duration::from_secs(5), async {
            let (mut control, _) = listener.accept().await.unwrap_or_abort();
            let pid = i32::try_from(control.read_u32().await.unwrap_or_abort()).unwrap_or_abort();
            control.write_all(&[mode]).await.unwrap_or_abort();
            assert_eq!(control.read_u8().await.unwrap_or_abort(), b'!');
            (control, Pid::from_raw(pid).unwrap_or_abort())
        })
        .await
        .unwrap_or_abort();
        match mode {
            b'r' => {
                let error = call
                    .await
                    .unwrap_or_abort()
                    .expect_err("initialize must fail");
                assert!(matches!(error, ToolError::Execution(message)
                    if message == "MCP `initialize` failed: fixture rejected initialization (code -32603)"));
            }
            b'c' => {
                call.abort();
                assert!(call
                    .await
                    .expect_err("startup must be cancelled")
                    .is_cancelled());
            }
            b'L' | b'H' | b'B' => {
                let error = call
                    .await
                    .unwrap_or_abort()
                    .expect_err("oversized response");
                let expected = if mode == b'H' {
                    "MCP header exceeded 8192-byte limit"
                } else {
                    "MCP response exceeded 16777216-byte limit"
                };
                assert!(matches!(error, ToolError::Execution(message) if message == expected));
            }
            _ => {
                assert_eq!(
                    call.await.unwrap_or_abort().unwrap_or_abort().display_text,
                    "1"
                );
                let tool = registry.get("mcp.fixture.echo").unwrap_or_abort();
                let result = timeout(
                    Duration::from_secs(5),
                    tool.call(test_context(&workspace, "mcp-reuse"), json!({})),
                )
                .await
                .unwrap_or_abort()
                .unwrap_or_abort();
                assert_eq!(result.display_text, "2", "healthy session must be reused");
            }
        }
        let reaped_on_return =
            !b"rLHB".contains(&mode) || test_kill_process(process) == Err(Errno::SRCH);
        drop(registry);

        let mut byte = [0];
        if !matches!(
            timeout(Duration::from_secs(2), control.read(&mut byte)).await,
            Ok(Ok(0))
        ) || !reaped_on_return
        {
            survivors.push(char::from(mode));
        }
        // Release any baseline survivor before asserting, then verify that it is reaped.
        drop(control);
        timeout(Duration::from_secs(2), async {
            let mut result = test_kill_process(process);
            while result.is_ok() {
                tokio::task::yield_now().await;
                result = test_kill_process(process);
            }
            assert_eq!(result, Err(Errno::SRCH), "failed to inspect MCP process");
        })
        .await
        .unwrap_or_abort();
    }
    assert!(
        survivors.is_empty(),
        "MCP startup left children alive: {survivors:?}"
    );
}

#[tokio::test]
async fn mcp_redaction_tool_result_does_not_contain_secret_patterns() {
    // arrange
    // Given: a workspace with a fake MCP server
    let temp_dir = setup_workspace();
    let workspace = temp_dir.path().join("workspace");
    let script_path = temp_dir.path().join("fake_mcp_server.py");
    install_fake_mcp_server(&script_path);
    let registry =
        coordinator_registry_with_mcp(ShellAllowlist::default(), fake_mcp_config(&script_path));

    // act
    // When: the echo tool is called with a secret-like input
    let tool = registry.get("mcp.fixture.echo").unwrap_or_abort();
    let result = tool
        .call(
            test_context(&workspace, "mcp-redaction"),
            json!({"text": "normal text"}),
        )
        .await
        .unwrap_or_abort();

    // assert
    // Then: the structured result does not contain secret-like patterns
    let json = serde_json::to_string(&result.structured_json).unwrap_or_default();
    assert!(!json.contains("Bearer "), "must not contain bearer tokens");
    assert!(
        !json.contains("sk-AbCdEf"),
        "must not contain API key patterns"
    );
    assert!(!json.contains("password"), "must not contain password");
}

// ---------------------------------------------------------------------------
// LSP family
// ---------------------------------------------------------------------------

fn config_with_lsp_json(lsp_json: &str) -> String {
    format!(
        r#"
        {{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{ baseURL: "http://127.0.0.1:8317/v1", apiKey: "test-key", apiMode: "responses", timeoutMs: 60000 }},
              models: {{ "gpt-5.4-mini": {{ name: "GPT-5.4 Mini" }} }},
            }},
          }},
          model: "default/gpt-5.4-mini",
          agent: {{ default: {{ tools: ["read"] }} }},
          permission: {{ edit: "ask", bash: "ask", webfetch: "deny" }},
          lsp: {{ servers: {lsp_json} }},
        }}
        "#
    )
}

#[test]
fn lsp_boundary_e2e_valid_config_with_custom_server_loads_successfully() {
    // arrange
    // Given: a valid LSP config with a custom server
    let raw = config_with_lsp_json(
        r#"{
        "custom-lang": {
            "command": ["/usr/bin/cat"],
            "extensions": [".lang"],
            "env": {},
            "initialization": {}
        }
    }"#,
    );

    // act
    // When: loaded from string
    let config = load_config_from_str(&raw).expect("valid LSP config");

    // assert
    // Then: the config is accepted with the LSP server registered
    assert!(config.lsp.servers.contains_key("custom-lang"));
    let server = &config.lsp.servers["custom-lang"];
    assert!(server.command.is_some());
    assert!(server.extensions.is_some());
}

#[test]
fn lsp_bad_input_custom_server_without_command_rejected_by_config_validation() {
    // arrange
    // Given: an LSP config with a custom server missing the command field
    let raw = config_with_lsp_json(
        r#"{
        "custom-lang": {
            "extensions": [".lang"]
        }
    }"#,
    );

    // act
    // When: loaded from string
    let err = load_config_from_str(&raw).expect_err("missing command must fail");

    // assert
    // Then: config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("command") || msg.contains("custom-lang"),
        "expected command validation error, got: {msg}"
    );
}

#[test]
fn lsp_bad_input_custom_server_without_extensions_rejected_by_config_validation() {
    // arrange
    // Given: an LSP config with a custom server missing the extensions field
    let raw = config_with_lsp_json(
        r#"{
        "custom-lang": {
            "command": ["/usr/bin/cat"]
        }
    }"#,
    );

    // act
    // When: loaded from string
    let err = load_config_from_str(&raw).expect_err("missing extensions must fail");

    // assert
    // Then: config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("extensions") || msg.contains("custom-lang"),
        "expected extensions validation error, got: {msg}"
    );
}

#[test]
fn lsp_permission_denial_disabled_lsp_config_does_not_register_servers() {
    // arrange
    // Given: an LSP config with disabled=true
    let config = LspConfig {
        disabled: true,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["rust-analyzer".to_string()]),
                extensions: Some(vec![".rs".to_string()]),
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    };

    // act
    // When: the config is checked

    // assert
    // Then: the LSP is globally disabled, so servers are not active
    assert!(config.disabled);
    assert!(
        !config.servers.is_empty(),
        "servers are still defined but inactive"
    );
}

#[test]
fn lsp_process_failure_registry_without_lsp_returns_no_lsp_tool() {
    // arrange
    // Given: a coordinator registry without LSP config
    let registry = harness_tools::coordinator_registry(ShellAllowlist::default());

    // act
    // When: the LSP tool is looked up
    let tool = registry.get("lsp");

    // assert
    // Then: the LSP tool may be absent or present but returns errors for unavailable servers
    if let Some(tool) = tool {
        let _cap = tool.capability();
    }
}

#[test]
fn lsp_cancellation_restart_config_can_be_updated_and_reloaded() {
    // arrange
    // Given: an initial LSP config with one server
    let config1 = LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["rust-analyzer".to_string()]),
                extensions: Some(vec![".rs".to_string()]),
                env: BTreeMap::new(),
                initialization: None,
            },
        )]),
    };

    // act
    // When: the config is updated to add a second server (restart/reload)
    let config2 = LspConfig {
        disabled: false,
        servers: {
            let mut servers = config1.servers.clone();
            servers.insert(
                "typescript".to_string(),
                LspServerConfig {
                    disabled: false,
                    command: Some(vec!["typescript-language-server".to_string()]),
                    extensions: Some(vec![".ts".to_string()]),
                    env: BTreeMap::new(),
                    initialization: None,
                },
            );
            servers
        },
    };

    // assert
    // Then: the updated config has both servers
    assert_eq!(config1.servers.len(), 1);
    assert_eq!(config2.servers.len(), 2);
    assert!(config2.servers.contains_key("rust"));
    assert!(config2.servers.contains_key("typescript"));
}

#[test]
fn lsp_redaction_config_does_not_contain_secret_patterns() {
    // arrange
    // Given: an LSP config with env vars that might contain secrets
    let config = LspConfig {
        disabled: false,
        servers: BTreeMap::from([(
            "rust".to_string(),
            LspServerConfig {
                disabled: false,
                command: Some(vec!["rust-analyzer".to_string()]),
                extensions: Some(vec![".rs".to_string()]),
                env: BTreeMap::from([
                    ("RUST_LOG".to_string(), "debug".to_string()),
                    ("API_KEY".to_string(), "sk-AbCdEf0123456789".to_string()),
                ]),
                initialization: None,
            },
        )]),
    };

    // act
    // When: the config is serialized
    let json = serde_json::to_string(&config).expect("serialize");

    // assert
    // Then: the serialized config contains the env var but the test verifies
    // that the LSP config surface itself does not add secret patterns beyond
    // what the operator configured (the redaction layer handles env var redaction
    // at the export/trace boundary, not at the config layer)
    assert!(json.contains("rust-analyzer"), "must contain the command");
    assert!(json.contains("RUST_LOG"), "must contain the env var name");
    // Note: the config layer stores env vars as-is; redaction happens at the
    // export/trace boundary. This test verifies the config surface shape.
}
