use super::*;

#[tokio::test]
async fn kernel_tools_are_reentrant_scoped_and_fenced_by_revision() -> Result {
    for language in ["js", "py"] {
        let (session, _root) = session(Settings::default())?;
        let tools = json!([{"name":"probe","parameters":{"type":"object"}}, {"name":"read","parameters":{"type":"object"}}]);
        let definition = if language == "js" {
            "tool(async function lookup(path) { return (await tool.read({path})).text; }, {description:'Read a path',schema:{type:'object',properties:{path:{type:'string'}},required:['path'],additionalProperties:false}}); await tool.probe({});"
        } else {
            "@tool\ndef lookup(path: str):\n    return tool.read(path=path)['text']\ntool.probe()"
        };
        let mut parent = session.execute("parent", json!({"language":language,"summary":"Define child tool while waiting","code":definition,"on_timeout":"error"}), tools.clone(), false).await?;
        let waiting = event(&mut parent, "call").await?;
        let described = session
            .describe_kernel_tools(&["lookup".into(), "absent".into()])
            .await?;
        assert_eq!(described["results"][0]["ok"], true, "{described}");
        assert_eq!(
            described["results"][1]["error"]["code"],
            "kernel_tool_missing"
        );
        let descriptor = &described["results"][0]["descriptor"];
        assert_eq!(descriptor["language"], language);
        assert_eq!(
            descriptor["input_schema"]["properties"]["path"]["type"],
            "string"
        );
        let request = json!({"name":"lookup","language":language,"kernel_generation":descriptor["kernel_generation"],"definition_revision":descriptor["definition_revision"],"args":{"path":"memo.txt"},"call_id":"child-read"});
        let mut invoked = session
            .invoke_kernel_tool(
                request.clone(),
                Some(json!({"tools":{"allow":["read"]}})),
                tools.clone(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        let nested = event(&mut invoked, "call").await?;
        assert_ne!(nested["parent"], waiting["parent"]);
        assert_eq!(nested["args"]["parameters"]["path"], "memo.txt");
        session.send(json!({"type":"reply","id":waiting["id"],"result":{"content":[{"type":"text","text":"parent finished"}],"details":{}}})).await?;
        assert_ne!(
            event(&mut parent, "result").await?["result"]["details"]["isError"],
            true
        );
        session.send(json!({"type":"reply","id":nested["id"],"result":{"content":[{"type":"text","text":"nested body"}],"details":{}}})).await?;
        assert_eq!(
            event(&mut invoked, "result").await?["result"],
            "nested body"
        );
        let wait_code = if language == "js" {
            "await tool.probe({});"
        } else {
            "tool.probe()"
        };
        let mut parent = session.execute("parent-again", json!({"language":language,"summary":"Wait while child tools run","code":wait_code,"on_timeout":"error"}), tools.clone(), false).await?;
        event(&mut parent, "call").await?;
        for scope in [
            json!({"tools":{"deny":["read"]}}),
            json!({"tools":{"allow":"malformed"}}),
        ] {
            let mut denied = session
                .invoke_kernel_tool(
                    request.clone(),
                    Some(scope),
                    tools.clone(),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await?;
            let denial = event(&mut denied, "error").await?;
            assert_eq!(
                denial["error"]["code"], "kernel_tool_host_denied",
                "{denial}"
            );
        }
        let mut interrupted = session
            .invoke_kernel_tool(
                request.clone(),
                None,
                tools.clone(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        event(&mut interrupted, "call").await?;
        session
            .send(json!({"type":"cancel","id":"parent-again"}))
            .await?;
        assert_eq!(
            event(&mut parent, "result").await?["result"]["details"]["cells"][0]["status"],
            "cancelled"
        );
        assert_eq!(
            event(&mut interrupted, "error").await?["error"]["code"],
            "kernel_tool_stale"
        );
        let cancelled = tokio_util::sync::CancellationToken::new();
        cancelled.cancel();
        let rejected = session
            .invoke_kernel_tool(request.clone(), None, tools.clone(), cancelled)
            .await;
        assert!(
            rejected.is_err(),
            "a pre-cancelled invocation must never enter the kernel"
        );
        let recursive = if language == "js" {
            "tool(async function recursive_agent() { return await agent('nested'); }); tool(async function recursive_pool() { return await workpool({category:'default',prompt:'nested'},'nested'); });"
        } else {
            "@tool\ndef recursive_agent():\n    return agent('nested')\n@tool\ndef recursive_pool():\n    return workpool({'category':'default','prompt':'nested'},'nested')"
        };
        run_language(&session, language, "recursive-definitions", recursive).await?;
        let descriptors = session
            .describe_kernel_tools(&["recursive_agent".into(), "recursive_pool".into()])
            .await?;
        for descriptor in descriptors["results"]
            .as_array()
            .ok_or("missing descriptors")?
        {
            let mut invocation = descriptor["descriptor"].clone();
            invocation["args"] = json!({});
            invocation["call_id"] = "recursion-check".into();
            let mut events = session
                .invoke_kernel_tool(
                    invocation,
                    None,
                    tools.clone(),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await?;
            assert_eq!(
                event(&mut events, "error").await?["error"]["code"],
                "kernel_tool_recursion"
            );
        }
        if language == "js" {
            let invalid = run(&session, "invalid-definition", "[() => 1, function bad(x=1){}, function* generator(){}].map(fn => {try {tool(fn); return 'accepted'} catch(error) {return error.code}})").await?;
            assert_eq!(
        invalid["content"][0]["text"],
        "[\"invalid_tool_definition\",\"invalid_tool_definition\",\"invalid_tool_definition\"]"
    );
        }
        let revised = run_language(
            &session,
            language,
            "redefine",
            if language == "js" {
                "if (!tool.defined().some(t=>t.name==='lookup')) throw Error('missing definition'); tool.undefine('lookup'); tool(function lookup(path) {return path});"
            } else {
                "assert any(t['name'] == 'lookup' for t in tool.defined())\ntool.undefine('lookup')\n@tool\ndef lookup(path: str):\n    return path"
            },
        )
        .await?;
        assert_ne!(revised["details"]["isError"], true, "{revised}");
        let mut stale = session
            .invoke_kernel_tool(
                request,
                None,
                tools,
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        assert_eq!(
            event(&mut stale, "error").await?["error"]["code"],
            "kernel_tool_stale"
        );
        session.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn scripts_and_package_revisions_preserve_kernel_state_and_failed_install_rollback() -> Result
{
    let (session, root) = session(Settings::default())?;
    let scripts = root.path().join("scripts with spaces");
    std::fs::create_dir_all(&scripts)?;
    std::fs::write(scripts.join("sibling.mjs"), "export default 2;")?;
    std::fs::write(
        scripts.join("loaded.mjs"),
        "import increment from './sibling.mjs'; var loaded = 40 + increment; loaded",
    )?;
    std::fs::write(scripts.join("sibling.py"), "increment = 2\n")?;
    std::fs::write(
        scripts.join("loaded.py"),
        "from sibling import increment\nloaded = 40 + increment\nloaded_file = __file__\nloaded\n",
    )?;
    for (language, filename, check) in [
        ("js", "loaded.mjs", "loaded"),
        ("py", "loaded.py", "assert '__file__' not in globals()\nassert 'scripts with spaces' not in __import__('sys').path[0]\nloaded"),
    ] {
        let loaded = run_language(&session, language, &format!("load-{language}"), &format!("%load {}", serde_json::to_string(&scripts.join(filename))?)).await?;
        assert_ne!(loaded["details"]["isError"], true, "{loaded}");
        assert!(loaded["content"][0]["text"].as_str().is_some_and(|s| s.contains("42")), "{loaded}");
        let kept = run_language(&session, language, &format!("keep-{language}"), check).await?;
        assert_ne!(kept["details"]["isError"], true, "{kept}");
        assert!(kept["content"][0]["text"].as_str().is_some_and(|s| s.contains("42")));
    }
    let package = root.path().join("fixture-package");
    std::fs::create_dir_all(&package)?;
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"harness-fixture","version":"1.0.0","type":"module","exports":"./index.mjs","scripts":{"install":"node -e \"require('fs').writeFileSync('INSTALL_RAN','bad')\""}}"#,
    )?;
    std::fs::write(package.join("index.mjs"), "export default 73;")?;
    let wheel = root.path().join("harness_fixture-1.0-py3-none-any.whl");
    let built = std::process::Command::new("python3").args(["-c", "import sys,zipfile\nwith zipfile.ZipFile(sys.argv[1], 'w') as z:\n z.writestr('harness_fixture.py', 'value = 73\\n')\n z.writestr('harness_fixture-1.0.dist-info/METADATA', 'Metadata-Version: 2.1\\nName: harness-fixture\\nVersion: 1.0\\n')\n z.writestr('harness_fixture-1.0.dist-info/WHEEL', 'Wheel-Version: 1.0\\nRoot-Is-Purelib: true\\nTag: py3-none-any\\n')\n z.writestr('harness_fixture-1.0.dist-info/RECORD', '')"])
        .arg(&wheel).output()?;
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    for (language, command, check, missing) in [
        (
            "js",
            "%npm install ./fixture-package".to_owned(),
            "var pkg = await import('harness-fixture'); [loaded,pkg.default]",
            "%npm install ./missing.tgz",
        ),
        (
            "py",
            format!(
                "%pip install {}",
                shell_words::quote(&wheel.to_string_lossy())
            ),
            "import harness_fixture\n[loaded, harness_fixture.value]",
            "%pip install ./missing.whl",
        ),
    ] {
        let installed =
            run_language(&session, language, &format!("install-{language}"), &command).await?;
        assert_ne!(installed["details"]["isError"], true, "{installed}");
        let kept = run_language(&session, language, &format!("import-{language}"), check).await?;
        assert_ne!(kept["details"]["isError"], true, "{kept}");
        assert!(
            kept["content"][0]["text"]
                .as_str()
                .is_some_and(|s| s.contains("73") && s.contains("42")),
            "{kept}"
        );
        let pointer = root
            .path()
            .join("local/environments")
            .join(language)
            .join("current.json");
        let before = std::fs::read(&pointer)?;
        let failed =
            run_language(&session, language, &format!("failed-{language}"), missing).await?;
        assert_eq!(failed["details"]["isError"], true, "{failed}");
        assert_eq!(std::fs::read(&pointer)?, before);
        let invalid = run_language(
            &session,
            language,
            &format!("flags-{language}"),
            &format!("{command} --prefix /tmp/escape"),
        )
        .await?;
        assert_eq!(invalid["details"]["isError"], true, "{invalid}");
        if language == "py" {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let cancel_id = format!("cancel-install-{language}");
            let slow = if language == "js" {
                format!("%npm install http://{address}/slow.tgz")
            } else {
                format!("%pip install http://{address}/slow-1.0-py3-none-any.whl")
            };
            let mut installing = session.execute(&cancel_id, json!({"language":language,"code":slow,"summary":"Cancel a blocked package download","on_timeout":"error"}), json!([]), false).await?;
            let (_connection, _) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::select! {
                connection = listener.accept() => Ok::<_, Box<dyn std::error::Error + Send + Sync>>(connection?),
                ended = event(&mut installing, "result") => Err(format!("{language} installer finished before connecting: {}", ended?).into()),
            }
        }).await.map_err(|_| format!("{language} installer did not reach the local download"))??;
            session
                .send(json!({"type":"cancel","id":cancel_id}))
                .await?;
            let cancelled = event(&mut installing, "result").await?;
            assert_eq!(
                cancelled["result"]["details"]["cells"][0]["status"], "cancelled",
                "{cancelled}"
            );
            assert_eq!(std::fs::read(&pointer)?, before);
        }
        let mut reset = session.execute(&format!("reset-packages-{language}"), json!({"language":language,"reset":true,"summary":"Import committed revision after reset","code":if language == "js" {"(await import('harness-fixture')).default"} else {"import harness_fixture\nharness_fixture.value"},"on_timeout":"error"}), json!([]), false).await?;
        let restored = event(&mut reset, "result").await?["result"].clone();
        assert_ne!(restored["details"]["isError"], true, "{restored}");
        assert!(
            restored["content"][0]["text"]
                .as_str()
                .is_some_and(|s| s.contains("73")),
            "{restored}"
        );
    }
    assert!(!package.join("INSTALL_RAN").exists());
    for file in ["node_modules", "package.json", "package-lock.json", ".venv"] {
        assert!(
            !root.path().join(file).exists(),
            "managed install modified {file}"
        );
    }
    session.close().await?;
    Ok(())
}

#[tokio::test]
async fn isolated_cells_fence_state_and_capabilities_and_survive_limits() -> Result {
    let mut settings = Settings::default();
    settings.sandbox.enabled = true;
    settings.sandbox.memory_limit_mb = 8;
    settings.sandbox.timeout_seconds = 1;
    let (session, _root) = session(settings)?;
    run(
        &session,
        "persistent",
        "var persistentValue = 73; persistentValue",
    )
    .await?;
    for (id, code, error) in [
        ("isolate-clean", "if ([typeof process,typeof require,typeof fetch,typeof setTimeout,typeof persistentValue].some(x=>x!=='undefined')) throw new Error('ambient access'); globalThis.privateValue=42; display(await Promise.resolve(privateValue));", false),
        ("isolate-next", "if (typeof privateValue!=='undefined') throw new Error('state leaked'); try{store('x',1);throw new Error('store worked')}catch(e){if(!e.message.includes('no persistent'))throw e} print(load('x')===undefined);", false),
        ("isolate-memory", "const arrays=[]; for(;;) arrays.push(new Array(100000).fill(42));", true),
        ("isolate-time", "for(;;){}", true),
        ("isolate-recovery", "display(6*7)", false),
    ] {
        let mut events = session.execute(id, json!({"language":"js","isolate":true,"code":code,"summary":"Verify isolated execution","on_timeout":"error"}), json!([]), false).await?;
        let result = event(&mut events, "result").await?["result"].clone();
        assert_eq!(result["details"]["isError"] == true, error, "{id}: {result}");
        assert_eq!(result["details"]["runtime"]["isolated"], true, "{result}");
    }
    let retained = run(&session, "persistent-retained", "persistentValue").await?;
    assert_eq!(retained["content"][0]["text"], "73");
    let tools = json!([{"name":"probe","parameters":{"type":"object"}}]);
    let mut isolated = session.execute("isolate-host", json!({"language":"js","isolate":true,"code":"display(await tool.probe({}));","summary":"Cancel a host wait","on_timeout":"error"}), tools, false).await?;
    event(&mut isolated, "call").await?;
    session
        .send(json!({"type":"cancel","id":"isolate-host"}))
        .await?;
    assert_eq!(
        event(&mut isolated, "result").await?["result"]["details"]["cells"][0]["status"],
        "cancelled"
    );
    session.close().await?;
    Ok(())
}
