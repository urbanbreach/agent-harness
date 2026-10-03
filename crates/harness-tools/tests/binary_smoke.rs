#![cfg(unix)]
use harness_core::{
    clock::FakeClock,
    config::ShellAllowlist,
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor, EventV1, ToolCallStatus},
    perm::PermissionPolicy,
    redact::DefaultRedactor,
};
use serde_json::json;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
include!("native/formatters.rs");
include!("native/ast_grep.rs");
include!("native/lsp_rename.rs");
include!("native/shell_sandbox.rs");
include!("native/command_stream.rs");

#[tokio::test]
async fn mcp_stdio_reuses_its_process_and_reaps_descendants_on_stop(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::config::{McpConfig, McpServerConfig};
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let root = tempfile::tempdir()?;
    let script = r#"import json, os, signal, subprocess, sys
child = subprocess.Popen(["sleep", "60"])
def stop(*unused):
    child.wait()
    open("closed", "w").write("reaped")
    sys.exit(0)
signal.signal(signal.SIGTERM, stop)
for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    if request["method"] == "initialize":
        result = {"protocolVersion":"2025-11-25", "capabilities":{"tools":{}}, "serverInfo":{"name":"fixture", "version":"1"}}
    else:
        result = {"content":[{"type":"text", "text":str(child.pid)}]}
    print(json.dumps({"jsonrpc":"2.0", "id":request["id"], "result":result}), flush=True)
child.wait()
"#;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_mcp_tools(
        &mut registry,
        McpConfig {
            servers: [(
                "native".into(),
                McpServerConfig::Stdio {
                    command: vec!["python3".into(), "-c".into(), script.into()],
                    env: Default::default(),
                    cwd: None,
                    timeout_secs: 3,
                    enabled: true,
                },
            )]
            .into(),
        },
    )?;
    let registry = Arc::new(registry);
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::clone(&registry);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    handle.start_run("stdio", root.path()).await?;
    let mut pids = Vec::new();
    for _ in 0..2 {
        let output = handle
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "mcp.native.tool.call",
                json!({"tool":"pid"}),
            )
            .await?;
        pids.push(output.display_text.parse::<i32>()?);
    }
    assert_eq!(pids[0], pids[1]);
    tokio::time::timeout(std::time::Duration::from_secs(3), handle.stop_run()).await??;
    assert_eq!(
        std::fs::read_to_string(root.path().join("closed"))?,
        "reaped"
    );
    assert_eq!(
        rustix::process::test_kill_process(
            rustix::process::Pid::from_raw(pids[0]).ok_or("invalid PID")?
        ),
        Err(rustix::io::Errno::SRCH)
    );
    drop(registry);
    Ok(())
}

#[tokio::test]
async fn lsp_reuses_synced_documents_and_reaps_cancelled_servers(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::config::{LspConfig, LspServerConfig};
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let root = tempfile::tempdir()?;
    let script = r#"import json, os, signal, socket, subprocess, sys
child = subprocess.Popen(["sleep", "60"])
def stop(*unused):
    child.wait()
    sys.exit(0)
signal.signal(signal.SIGTERM, stop)
def read():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line: raise EOFError()
        if line == b"\r\n": break
        key, value = line.decode().split(":", 1)
        headers[key.lower()] = value.strip()
    return json.loads(sys.stdin.buffer.read(int(headers["content-length"])))
def send(value):
    body = json.dumps(dict(jsonrpc="2.0", **value)).encode()
    sys.stdout.buffer.write(("Content-Length: %d\r\n\r\n" % len(body)).encode() + body)
    sys.stdout.buffer.flush()
documents = {}
while True:
    req = read()
    method = req.get("method")
    args = req.get("params", {})
    if method == "initialize":
        assert args["capabilities"]["general"]["positionEncodings"] == ["utf-16"]
        result = {"capabilities":{"hoverProvider":True,"textDocumentSync":2,"diagnosticProvider":{"workspaceDiagnostics":True},"workspaceSymbolProvider":True,"callHierarchyProvider":True,"positionEncoding":"utf-16"}}
    elif method == "textDocument/didOpen":
        doc = args["textDocument"]
        documents[doc["uri"]] = doc["text"]
        continue
    elif method == "textDocument/didChange":
        assert args["contentChanges"][0]["range"]["end"] == {"line":1,"character":0}
        documents[args["textDocument"]["uri"]] = args["contentChanges"][0]["text"]
        continue
    elif method == "textDocument/hover":
        assert args["position"] == {"line":0,"character":2}
        send({"id":"apply", "method":"workspace/applyEdit", "params":{"edit":{"changes":{}}}})
        assert read()["result"]["applied"] is False
        result = {"contents":{"kind":"plaintext","value":documents[args["textDocument"]["uri"]]}, "pid":child.pid}
    elif method == "textDocument/diagnostic":
        result = {"kind":"full","items":[{"range":{"start":{"line":0,"character":2},"end":{"line":0,"character":5}},"message":"fixture warning","severity":2}]}
    elif method == "textDocument/prepareCallHierarchy":
        result = None
    elif method == "workspace/symbol":
        sock = socket.socket(socket.AF_UNIX)
        sock.connect("ready.sock")
        sock.sendall(str(child.pid).encode())
        sock.close()
        continue
    elif method == "shutdown":
        result = None
    elif method == "exit":
        sys.exit(0)
    else:
        continue
    send({"id":req["id"],"result":result})
"#;
    harness_core::config::set_registered_lsp_config(LspConfig {
        disabled: false,
        servers: [(
            "fixture".into(),
            LspServerConfig {
                command: Some(vec!["python3".into(), "-c".into(), script.into()]),
                extensions: Some(vec![".rs".into()]),
                ..Default::default()
            },
        )]
        .into(),
    });
    let registry = Arc::new(harness_tools::coordinator_registry(
        ShellAllowlist::default(),
    ));
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = registry;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let info = coordinator.start_run("lsp", root.path()).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    let mut pid = None;
    for text in ["\u{1f600}old\n", "\u{1f600}new\n"] {
        std::fs::write(root.path().join("a.rs"), text)?;
        let output = coordinator
            .execute_agent_tool_call(
                actor(),
                None,
                "lsp",
                json!({
                    "operation":"hover", "filePath":"a.rs", "line":1, "character":3
                }),
            )
            .await?;
        assert!(!output.is_error(), "{}", output.display_text);
        let result = output.structured_json.ok_or("missing structured result")?;
        assert_eq!(result["result"]["contents"]["value"], text);
        let current = result["result"]["pid"].as_i64().ok_or("missing PID")?;
        if let Some(pid) = pid {
            assert_eq!(pid, current, "reuse the server");
        }
        pid = Some(current);
    }
    let diagnostic = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "lsp",
            json!({
                "operation":"fileDiagnostics", "filePath":"a.rs"
            }),
        )
        .await?;
    assert!(diagnostic.display_text.contains("fixture warning"));
    let calls = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "lsp",
            json!({
                "operation":"incomingCalls", "filePath":"a.rs", "line":1, "character":3
            }),
        )
        .await?;
    assert_eq!(
        calls.structured_json.ok_or("missing call hierarchy")?["result"],
        json!([])
    );
    let invalid = coordinator
        .execute_agent_tool_call(
            actor(),
            None,
            "lsp",
            json!({
                "operation":"hover", "filePath":"a.rs", "line":1, "character":2
            }),
        )
        .await;
    assert!(invalid.is_err(), "reject a UTF-16 surrogate midpoint");
    let listener = tokio::net::UnixListener::bind(root.path().join("ready.sock"))?;
    let task = coordinator
        .request_tool_call(
            actor(),
            None,
            "lsp",
            json!({
                "operation":"workspaceSymbol", "filePath":"a.rs", "query":"block"
            }),
        )
        .await?;
    let pid = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let (mut socket, _) = listener.accept().await?;
        let mut text = String::new();
        socket.read_to_string(&mut text).await?;
        Ok::<_, std::io::Error>(text)
    })
    .await??;
    coordinator.cancel_task(task, "cancel LSP request").await?;
    tokio::time::timeout(std::time::Duration::from_secs(3), coordinator.stop_run()).await??;
    assert_eq!(
        rustix::process::test_kill_process(
            rustix::process::Pid::from_raw(pid.parse()?).ok_or("invalid PID")?
        ),
        Err(rustix::io::Errno::SRCH)
    );
    assert!(
        !harness_core::store::read_events(&info.events_path)?
            .iter()
            .any(|event| matches!(event.payload, EventV1::EditApplied(_))),
        "server requests cannot bypass edit policy"
    );
    Ok(())
}

#[tokio::test]
async fn rust_analyzer_resolves_a_symbol_through_the_native_lsp_tool(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::config::{LspConfig, LspServerConfig};
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for real process checks".into());
    }
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join("src"))?;
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"lsp_probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn answer() -> u32 { 42 }\npub fn use_answer() -> u32 { answer() }\n",
    )?;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_lsp_tools(&mut registry, LspConfig {
        servers:[("rust".into(), LspServerConfig { initialization:Some(json!({"checkOnSave":false,"cargo":{"sysroot":null},"procMacro":{"enable":false}})), ..Default::default() })].into(),
        ..Default::default()
    });
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.permission_policy = PermissionPolicy::allow_all();
    config.tool_registry = Arc::new(registry);
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    coordinator.start_run("rust-analyzer", root.path()).await?;
    let outcome = async {
        let symbols = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "lsp",
                json!({
                    "operation":"documentSymbol", "filePath":"src/lib.rs"
                }),
            )
            .await?;
        let symbols = symbols.structured_json.ok_or("missing symbols")?;
        assert!(
            symbols["result"]
                .as_array()
                .ok_or("invalid symbols")?
                .iter()
                .any(|item| item["name"] == "answer"),
            "{symbols}"
        );
        let definition = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "lsp",
                json!({
                    "operation":"goToDefinition", "filePath":"src/lib.rs", "line":2, "character":30
                }),
            )
            .await?;
        let definition = definition.structured_json.ok_or("missing definition")?;
        let result = definition["result"]
            .as_array()
            .ok_or("invalid definition")?;
        assert!(
            result
                .iter()
                .any(|item| item["range"]["start"]["line"] == 0),
            "{definition}"
        );
        let renamed = coordinator.execute_agent_tool_call(
            EventActor::new(ActorKind::User, None), None, "lsp.rename",
            json!({"filePath":"src/lib.rs","line":1,"character":8,"newName":"result","apply":true}),
        ).await?;
        assert_eq!(
            renamed.structured_json.ok_or("missing rename result")?["applied"],
            true
        );
        let updated = std::fs::read_to_string(root.path().join("src/lib.rs"))?;
        assert!(updated.contains("fn result()"), "{updated}");
        assert!(updated.contains("{\n    result()"), "{updated}");
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    coordinator.stop_run().await?;
    outcome
}
