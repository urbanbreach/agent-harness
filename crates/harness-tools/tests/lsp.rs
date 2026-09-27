use harness_core::{
    clock::FakeClock,
    config::{LspConfig, LspServerConfig, ShellAllowlist},
    coord::{spawn_coordinator, CoordinatorConfig},
    event::{ActorKind, EventActor},
    perm::{PermissionAction, PermissionPolicy, PermissionRule},
    redact::DefaultRedactor,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn lsp_requires_both_read_and_lsp_permissions_before_starting_a_server(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("secret.rs"), "fn main() {}")?;
    std::fs::create_dir(root.path().join("alias"))?;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_lsp_tools(
        &mut registry,
        LspConfig {
            disabled: false,
            servers: [(
                "fixture".into(),
                LspServerConfig {
                    command: Some(vec!["this-command-must-never-be-started".into()]),
                    extensions: Some(vec!["rs".into()]),
                    ..Default::default()
                },
            )]
            .into(),
        },
    );
    let registry = Arc::new(registry);
    for permission in ["lsp", "read"] {
        let mut config = CoordinatorConfig::new(root.path().join("sessions"));
        config.tool_registry = Arc::clone(&registry);
        config.permission_policy = PermissionPolicy::from_rules(vec![
            PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: permission.into(),
                pattern: "secret.rs".into(),
                action: PermissionAction::Deny,
            },
        ])?;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run("policy", root.path()).await?;
        let result = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "lsp",
                json!({
                    "operation":"hover", "file_path":"alias/../secret.rs", "line":1, "character":1
                }),
            )
            .await;
        assert!(
            result.is_err_and(|error| error.contains("denied")),
            "{permission} must be checked before starting LSP"
        );
        coordinator.stop_run().await?;
    }
    Ok(())
}

#[tokio::test]
async fn installation_choices_are_immutable_receipts_and_do_not_start_a_server(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_lsp_tools(
        &mut registry,
        LspConfig {
            disabled: true,
            ..Default::default()
        },
    );
    let mut config = CoordinatorConfig::new(root.path().join("sessions"));
    config.tool_registry = Arc::new(registry);
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "lsp".into(),
            pattern: "forbidden-server".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("installation choices", root.path())
        .await?;
    let mut receipts = Vec::new();
    for decision in ["declined", "allowed"] {
        let result = coordinator.execute_agent_tool_call(EventActor::new(ActorKind::User, None), None, "lsp", json!({"operation":"installDecision","serverId":"uninstalled-server","decision":decision})).await?;
        assert_eq!(result.artifacts.len(), 1);
        let path = run.run_dir.join(&result.artifacts[0].path);
        let receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(receipt["decision"], decision);
        assert_eq!(receipt["recorded_only"], true);
        receipts.push(path);
    }
    for args in [
        json!({"operation":"installDecision","serverId":"forbidden-server","decision":"allowed"}),
        json!({"operation":"installDecision","serverId":"uninstalled-server","decision":"yes"}),
        json!({"operation":"installDecision","decision":"allowed"}),
        json!({"operation":"hover","line":1,"character":1}),
    ] {
        assert!(coordinator
            .execute_agent_tool_call(EventActor::new(ActorKind::User, None), None, "lsp", args)
            .await
            .is_err());
    }
    coordinator.stop_run().await?;
    assert_ne!(receipts[0], receipts[1]);
    assert!(std::fs::read_to_string(&receipts[0])?.contains("declined"));
    assert_eq!(
        harness_core::store::read_events(&run.events_path)?
            .iter()
            .filter(|e| matches!(e.payload, harness_core::event::EventV1::ArtifactWritten(_)))
            .count(),
        2
    );
    assert_eq!(std::fs::read_dir(root.path())?.count(), 1);
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_diagnostics_fall_back_to_files_without_sending_denied_contents(
) -> Result<(), Box<dyn std::error::Error>> {
    let script = r#"import json, pathlib, sys
mode = sys.argv[1]
def read():
    size = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line: raise EOFError()
        if line == b'\r\n': break
        key, value = line.decode().split(':', 1)
        if key.lower() == 'content-length': size = int(value)
    return json.loads(sys.stdin.buffer.read(size))
def send(value):
    body = json.dumps(dict(jsonrpc='2.0', **value)).encode()
    sys.stdout.buffer.write(('Content-Length: %d\r\n\r\n' % len(body)).encode() + body)
    sys.stdout.buffer.flush()
def diagnostics(uri):
    return [{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':1}},'message':uri,'severity':2}]
while True:
    req = read()
    method = req.get('method')
    params = req.get('params', {})
    if method == 'initialize':
        caps = {'textDocumentSync':1}
        if mode == 'pull': caps['diagnosticProvider'] = {'workspaceDiagnostics':False}
        result = {'capabilities':caps}
    elif method == 'textDocument/didOpen':
        document = params['textDocument']
        assert 'DENIED' not in document['text']
        with open('opened', 'a') as f: f.write(document['uri'] + '\n')
        if mode == 'push': send({'method':'textDocument/publishDiagnostics','params':{'uri':document['uri'],'version':document['version'],'diagnostics':diagnostics(document['uri'])}})
        continue
    elif method == 'textDocument/diagnostic':
        assert mode == 'pull'
        result = {'kind':'full','items':diagnostics(params['textDocument']['uri'])}
    elif method == 'shutdown': result = None
    elif method == 'exit': break
    else: continue
    send({'id':req['id'],'result':result})
"#;
    for mode in ["pull", "push"] {
        let root = tempfile::tempdir()?;
        for (name, text) in [
            ("entry", "entry"),
            ("other", "other"),
            ("denied_read", "DENIED"),
            ("denied_lsp", "DENIED"),
        ] {
            std::fs::write(root.path().join(format!("{name}.fixture")), text)?;
        }
        let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
        harness_tools::register_lsp_tools(
            &mut registry,
            LspConfig {
                servers: [(
                    "fixture".into(),
                    LspServerConfig {
                        command: Some(vec![
                            "python3".into(),
                            "-c".into(),
                            script.into(),
                            mode.into(),
                        ]),
                        extensions: Some(vec!["fixture".into()]),
                        ..Default::default()
                    },
                )]
                .into(),
                ..Default::default()
            },
        );
        let mut config = CoordinatorConfig::new(root.path().join("sessions"));
        config.tool_registry = Arc::new(registry);
        config.permission_policy = PermissionPolicy::from_rules(vec![
            PermissionRule {
                permission: "*".into(),
                pattern: "*".into(),
                action: PermissionAction::Allow,
            },
            PermissionRule {
                permission: "read".into(),
                pattern: "denied_read*.fixture".into(),
                action: PermissionAction::Deny,
            },
            PermissionRule {
                permission: "lsp".into(),
                pattern: "denied_lsp*.fixture".into(),
                action: PermissionAction::Deny,
            },
        ])?;
        let coordinator = spawn_coordinator(
            config,
            Arc::new(FakeClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        coordinator.start_run(mode, root.path()).await?;
        let result = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "lsp",
                json!({"operation":"workspaceDiagnostics","filePath":"entry.fixture"}),
            )
            .await?;
        let result = result.structured_json.ok_or("missing diagnostics")?;
        let result = &result["result"];
        assert_eq!(result["filesScanned"], 2, "{mode}: {result}");
        assert_eq!(result["skippedFiles"], 2);
        assert_eq!(result["complete"], false);
        assert_eq!(result["diagnosticCount"], 2);
        assert_eq!(result["reports"].as_array().map(Vec::len), Some(2));
        let opened = std::fs::read_to_string(root.path().join("opened"))?;
        assert!(!opened.contains("denied_"));
        assert_eq!(opened.lines().count(), 2);
        let edited = coordinator
            .execute_agent_tool_call(
                EventActor::new(ActorKind::User, None),
                None,
                "write",
                json!({"path":"fresh.fixture","content":"fresh"}),
            )
            .await?;
        assert!(
            edited.structured_json.ok_or("missing edit diagnostics")?["diagnostics"]
                ["fresh.fixture"]
                .to_string()
                .contains("severity")
        );
        for path in ["denied_read_new.fixture", "denied_lsp_new.fixture"] {
            let edited = coordinator
                .execute_agent_tool_call(
                    EventActor::new(ActorKind::User, None),
                    None,
                    "write",
                    json!({"path":path,"content":"DENIED"}),
                )
                .await?;
            assert_eq!(
                edited
                    .structured_json
                    .ok_or("missing unavailable diagnostics")?["diagnostics"][path]["unavailable"],
                true
            );
            assert_eq!(std::fs::read_to_string(root.path().join(path))?, "DENIED");
        }
        coordinator.stop_run().await?;
        let opened = std::fs::read_to_string(root.path().join("opened"))?;
        assert!(!opened.contains("denied_"));
        assert_eq!(opened.lines().count(), 3);
    }
    Ok(())
}
