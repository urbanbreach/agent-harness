use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(crate) fn install_fake_mcp_server_with_tools(script_path: &Path, tools_literal: &str) {
    let script = r#"#!/usr/bin/env python3
import json
import sys

TOOLS = __TOOLS_LITERAL__
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
        known_tool_names = {tool["name"] for tool in TOOLS}
        if tool_name in known_tool_names:
            send({
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {
                    "content": [{"type": "text", "text": arguments.get("text", f"called {tool_name}") }],
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
"#
    .replace("__TOOLS_LITERAL__", tools_literal);
    fs::write(script_path, script).expect("write fake mcp server");
    let mut permissions = fs::metadata(script_path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions).expect("chmod fake mcp server");
}

pub(crate) fn install_fake_mcp_server(script_path: &Path) {
    install_fake_mcp_server_with_tools(
        script_path,
        r#"[{
    "name": "echo",
    "description": "Echoes text input",
    "inputSchema": {
        "type": "object",
        "properties": {
            "text": {"type": "string"}
        }
    }
}]"#,
    );
}

pub(crate) fn install_stateful_terminal_mcp_server(script_path: &Path) {
    let script = r#"#!/usr/bin/env python3
import json
import sys

TOOLS = [{
    "name": "terminal_spawn",
    "description": "Spawns a terminal session",
    "inputSchema": {"type": "object", "properties": {"shell": {"type": "string"}}}
}, {
    "name": "terminal_wait",
    "description": "Waits for terminal output",
    "inputSchema": {"type": "object", "properties": {"sessionId": {"type": "string"}, "ms": {"type": "number"}}}
}]
SESSIONS = {}
NEXT_ID = 1


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
                "capabilities": {"tools": {"listChanged": False}}
            }
        })
        continue

    if method == "notifications/initialized":
        continue

    if message_id is None:
        continue

    if method == "tools/list":
        send({"jsonrpc": "2.0", "id": message_id, "result": {"tools": TOOLS}})
        continue

    if method != "tools/call":
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "error": {"code": -32601, "message": f"unsupported method: {method}"}
        })
        continue

    tool_name = params.get("name")
    arguments = params.get("arguments", {})
    if tool_name == "terminal_spawn":
        session_id = f"term-{NEXT_ID}"
        NEXT_ID += 1
        SESSIONS[session_id] = True
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {"content": [{"type": "text", "text": json.dumps({"sessionId": session_id})}], "isError": False}
        })
    elif tool_name == "terminal_wait":
        session_id = arguments.get("sessionId")
        if session_id not in SESSIONS:
            send({
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {"content": [{"type": "text", "text": f"No terminal session with id: {session_id}"}], "isError": True}
            })
        else:
            send({
                "jsonrpc": "2.0",
                "id": message_id,
                "result": {"content": [{"type": "text", "text": "terminal session still active"}], "isError": False}
            })
    else:
        send({
            "jsonrpc": "2.0",
            "id": message_id,
            "result": {"content": [{"type": "text", "text": f"unknown tool: {tool_name}"}], "isError": True}
        })
"#;
    fs::write(script_path, script).expect("write stateful mcp server");
    let mut permissions = fs::metadata(script_path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(script_path, permissions).expect("chmod stateful mcp server");
}
