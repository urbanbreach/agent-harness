#[tokio::test]
async fn semantic_rename_previews_checks_discovered_paths_and_applies_ordered_edits(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::{
        config::{FormatterConfig, LspConfig, LspServerConfig},
        event::PermissionDecision,
        perm::{PermissionAction, PermissionRule},
    };
    if std::env::var("HARNESS_BINARY_SIGNOFF").as_deref() != Ok("1") {
        return Err("set HARNESS_BINARY_SIGNOFF=1 for native LSP rename checks".into());
    }
    let root = tempfile::tempdir()?;
    let source = "\u{feff}😀old\r\n";
    std::fs::write(root.path().join("a.fixture"), source)?;
    std::fs::write(root.path().join("b.fixture"), "old\n")?;
    let script = r#"import json, pathlib, sys
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
while True:
    req = read()
    method, p = req.get('method'), req.get('params', {})
    if method == 'initialize':
        result = {'capabilities':{'textDocumentSync':1,'renameProvider':{'prepareProvider':True},'diagnosticProvider':{}}}
    elif method == 'textDocument/prepareRename':
        assert p['position'] == {'line':0,'character':3}
        result = {'range':{'start':{'line':0,'character':3},'end':{'line':0,'character':6}},'placeholder':'old'}
    elif method == 'textDocument/rename':
        assert p['newName'] == 'new'
        result = json.loads(pathlib.Path('plan.json').read_text())
    elif method == 'textDocument/diagnostic': result = {'kind':'full','items':[]}
    elif method == 'shutdown': result = None
    elif method == 'exit': break
    else: continue
    send({'id':req['id'],'result':result})
"#;
    let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
    harness_tools::register_lsp_tools(
        &mut registry,
        LspConfig {
            servers: [(
                "fixture".into(),
                LspServerConfig {
                    command: Some(vec!["python3".into(), "-c".into(), script.into()]),
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
    config.formatter = Arc::new(FormatterConfig {
        enabled: false,
        ..Default::default()
    });
    config.permission_policy = PermissionPolicy::from_rules(vec![
        PermissionRule {
            permission: "*".into(),
            pattern: "*".into(),
            action: PermissionAction::Allow,
        },
        PermissionRule {
            permission: "read".into(),
            pattern: "b.fixture".into(),
            action: PermissionAction::Ask,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: "b.fixture".into(),
            action: PermissionAction::Ask,
        },
        PermissionRule {
            permission: "edit".into(),
            pattern: "denied.fixture".into(),
            action: PermissionAction::Deny,
        },
    ])?;
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = handle.start_run("rename", root.path()).await?;
    let actor = || EventActor::new(ActorKind::User, None);
    let uri = |name: &str| {
        reqwest::Url::from_file_path(root.path().join(name))
            .map(String::from)
            .map_err(|()| "file URI")
    };
    let edit = |start: u32, end: u32, text: &str| json!({"range":{"start":{"line":0,"character":start},"end":{"line":0,"character":end}},"newText":text});
    let set_plan =
        |plan: serde_json::Value| std::fs::write(root.path().join("plan.json"), plan.to_string());
    let mut args =
        json!({"filePath":"a.fixture","line":1,"character":4,"newName":"new","apply":false});
    set_plan(json!({"changes":{uri("a.fixture")?:[edit(3,6,"new")]}}))?;
    let preview = handle
        .execute_agent_tool_call(actor(), None, "lsp.rename", args.clone())
        .await?;
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.fixture"))?,
        source
    );
    assert_eq!(
        preview.structured_json.as_ref().ok_or("missing preview")?["applied"],
        false
    );
    let diff = preview
        .artifacts
        .first()
        .ok_or("missing rename preview diff")?;
    assert!(std::fs::read_to_string(run.run_dir.join(&diff.path))?.contains("new"));

    let plan = json!({"documentChanges":[
        {"textDocument":{"uri":uri("a.fixture")?,"version":1},"edits":[edit(3,6,"new")]},
        {"textDocument":{"uri":uri("b.fixture")?,"version":null},"edits":[edit(0,3,"new")]}
    ]});
    set_plan(plan.clone())?;
    args["apply"] = true.into();
    let mut events = handle.subscribe_new_events().await?;
    for outcome in ["deny", "cancel", "stale", "allow"] {
        let task = handle
            .request_tool_call(actor(), None, "lsp.rename", args.clone())
            .await?;
        let read = ast_permission(&mut events).await?;
        assert!(read.summary.contains("b.fixture"));
        handle
            .resolve_permission(read.permission_id, PermissionDecision::Allow, None)
            .await?;
        let permission = ast_permission(&mut events).await?;
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.fixture"))?,
            source
        );
        if outcome == "cancel" {
            handle.cancel_task(&task, "cancel rename approval").await?;
        } else {
            if outcome == "stale" {
                std::fs::write(root.path().join("b.fixture"), "editor\n")?;
            }
            handle
                .resolve_permission(
                    permission.permission_id,
                    if outcome == "deny" {
                        PermissionDecision::Deny
                    } else {
                        PermissionDecision::Allow
                    },
                    None,
                )
                .await?;
        }
        let status = ast_finished(&mut events, &task).await?;
        assert_eq!(
            status,
            if outcome == "allow" {
                ToolCallStatus::Succeeded
            } else {
                ToolCallStatus::Failed
            }
        );
        if outcome == "stale" {
            assert_eq!(
                std::fs::read_to_string(root.path().join("b.fixture"))?,
                "editor\n"
            );
            std::fs::write(root.path().join("b.fixture"), "old\n")?;
        }
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.fixture"))?,
        "\u{feff}😀new\r\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("b.fixture"))?,
        "new\n"
    );

    // Invalid later operations must be rejected before the first write.
    for change in [
        json!({"textDocument":{"uri":uri("a.fixture")?,"version":999},"edits":[edit(3,6,"bad")]}),
        json!({"textDocument":{"uri":uri("a.fixture")?,"version":null},"edits":[edit(2,6,"bad")]}),
        json!({"textDocument":{"uri":uri("a.fixture")?,"version":null},"edits":[edit(3,6,"bad"),edit(4,6,"overlap")]}),
        json!({"kind":"create","uri":uri("denied.fixture")?}),
        json!({"kind":"delete","uri":"file:///outside-workspace.fixture"}),
    ] {
        set_plan(
            json!({"documentChanges":[{"textDocument":{"uri":uri("a.fixture")?,"version":null},"edits":[edit(3,6,"first")]},change]}),
        )?;
        assert!(handle
            .execute_agent_tool_call(actor(), None, "lsp.rename", args.clone())
            .await
            .is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.fixture"))?,
            "\u{feff}😀new\r\n"
        );
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(root.path().join("mode.fixture"), "same\n")?;
    std::fs::set_permissions(
        root.path().join("mode.fixture"),
        std::fs::Permissions::from_mode(0o751),
    )?;
    std::fs::write(root.path().join("target.fixture"), "same\n")?;
    std::fs::set_permissions(
        root.path().join("target.fixture"),
        std::fs::Permissions::from_mode(0o600),
    )?;
    set_plan(json!({"documentChanges":[
        {"kind":"rename","oldUri":uri("mode.fixture")?,"newUri":uri("target.fixture")?,"options":{"overwrite":true}},
        {"kind":"create","uri":uri("created.fixture")?},
        {"textDocument":{"uri":uri("created.fixture")?,"version":null},"edits":[edit(0,0,"left\r\n😀old\r\n")]},
        {"textDocument":{"uri":uri("created.fixture")?,"version":null},"edits":[{"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":5}},"newText":"new"}]},
        {"kind":"rename","oldUri":uri("created.fixture")?,"newUri":uri("moved.fixture")?},
        {"kind":"delete","uri":uri("a.fixture")?}
    ]}))?;
    let applied = handle
        .execute_agent_tool_call(actor(), None, "lsp.rename", args)
        .await?;
    assert_eq!(
        applied.structured_json.ok_or("missing application")?["applied"],
        true
    );
    assert_eq!(
        std::fs::metadata(root.path().join("target.fixture"))?
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
    assert!(!root.path().join("mode.fixture").exists());
    assert!(!root.path().join("created.fixture").exists());
    assert!(!root.path().join("a.fixture").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("moved.fixture"))?,
        "left\r\n😀new\r\n"
    );
    let journal = harness_core::store::read_events(&run.events_path)?;
    for snapshot in journal
        .iter()
        .rev()
        .filter_map(|event| match &event.payload {
            EventV1::WorkspaceSnapshot(s) => Some(s.request_id.to_string()),
            _ => None,
        })
    {
        handle.revert_workspace(snapshot).await?;
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.fixture"))?,
        source
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("b.fixture"))?,
        "old\n"
    );
    assert!(!root.path().join("moved.fixture").exists());
    handle.stop_run().await?;
    Ok(())
}
