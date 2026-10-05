use harness_eval::{Session, SessionOptions, Settings};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::mpsc;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn session(settings: Settings) -> Result<(Session, tempfile::TempDir)> {
    let root = tempfile::tempdir()?;
    let session = Session::new(SessionOptions {
        environment: BTreeMap::from([(
            "PATH".into(),
            std::env::var_os("PATH").unwrap_or_default(),
        )]),
        cwd: root.path().into(),
        artifacts: root.path().join("artifacts"),
        local_dir: root.path().join("local"),
        languages: vec!["js".into()],
        session_env: BTreeMap::new(),
        settings,
    })?;
    Ok((session, root))
}

async fn event(events: &mut mpsc::Receiver<Value>, kind: &str) -> Result<Value> {
    let mut last = Value::Null;
    tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(event) = events.recv().await {
            if event["type"] == kind {
                return Ok(event);
            }
            last = event;
        }
        Err(format!("eval stream closed before {kind}").into())
    })
    .await
    .map_err(|_| format!("timed out waiting for {kind}; last event: {last}"))?
}

async fn run(session: &Session, id: &str, code: &str) -> Result<Value> {
    let mut events = session.execute(id, json!({"language":"js","summary":"Verify native JavaScript","code":code,"on_timeout":"error"}), json!([]), false).await?;
    Ok(event(&mut events, "result").await?["result"].clone())
}

#[tokio::test]
async fn javascript_cells_preserve_language_semantics_and_output() -> Result {
    let (session, root) = session(Settings::default())?;
    std::fs::write(
        root.path().join("local-module.mjs"),
        "export const answer = 42; export default 7;",
    )?;
    std::fs::write(root.path().join("common.cjs"), "exports.answer = 42;")?;
    std::fs::write(
        root.path().join("typed.ts"),
        "export const answer: number = 42;",
    )?;
    let package = root.path().join("node_modules/eval-fixture");
    std::fs::create_dir_all(&package)?;
    std::fs::write(
        package.join("package.json"),
        r#"{"type":"module","exports":{"import":"./entry.mjs"}}"#,
    )?;
    std::fs::write(package.join("entry.mjs"), "export default 42;")?;
    let cases = [
        ("var value = 40; value + 2", "42\n"),
        ("value += 2; value", "42\n"),
        ("const {a: renamed, missing = 5, ...rest} = {a:7, b:9}; [renamed,missing,rest]", "[7,5,{\"b\":9}]\n"),
        ("let [first,,...tail] = [1,2,3,4]; [first,tail]", "[1,[3,4]]\n"),
        ("var consumed = 0; var [kept,,] = {[Symbol.iterator]:function*(){for(let i=0;i<3;i++){consumed++;yield i;}}}; [kept,consumed]", "[0,2]"),
        ("[renamed,first,value]", "[7,1,42]\n"),
        ("await Promise.resolve(6 * 7)", "42\n"),
        ("let pair = `return; ${/a;b/.test('a;b')}`; pair", "\"return; true\"\n"),
        ("import path from 'node:path'; path.basename('/a/b')", "\"b\"\n"),
        ("import def, {answer as imported} from './local-module.mjs'; [def, imported]", "[7,42]\n"),
        ("[path.extname('a.rs'),imported]", "[\".rs\",42]\n"),
        ("import common, {answer as cjsAnswer} from './common.cjs'; [common.answer,cjsAnswer]", "[42,42]"),
        ("import {answer as tsAnswer} from './typed.ts'; tsAnswer", "42"),
        ("import packageAnswer from 'eval-fixture'; packageAnswer", "42"),
        ("import {writeSync} from 'node:fs'; print('before native writes'); writeSync(1,'raw stdout\\n'); writeSync(2,'raw stderr\\n'); print('after native writes');", "before native writes\nraw stdout\nraw stderr\nafter native writes"),
        ("import {spawnSync} from 'node:child_process'; spawnSync('sh',['-c','printf child'],{stdio:'inherit'}); print(' process');", "child process"),
        ("return 4; 99", "4\n"),
        ("if(false) return 1; 2", "(no output)"),
        ("const declared = await Promise.resolve(42); // retained comment", "42\n"),
        ("print('before'); display({answer:42}); print('after');", "before\ndisplay[1]:\n{\n  \"answer\": 42\n}\nafter\n"),
        ("await pipeline([1,2,3], n => n * 2, n => n + 1)", "[3,5,7]\n"),
        ("await write('local://memo.txt','one\\ntwo\\nthree'); await read('local://memo.txt',{offset:2,limit:1})", "\"two\"\n"),
        ("await read('local://memo.txt',2,1)", "\"two\""),
        ("await write('local://blob.txt',new Blob(['blob text'])); await read('local://blob.txt')", "\"blob text\""),
        ("await parallel([index=>index,index=>index])", "[0,1]"),
        ("await parallel(null)", "[]"),
        ("await tool_schema(null)", "{\"tools\":[]}"),
        ("try {await workpool('task','test')} catch(error) {print(error.code)}", "workpool_unavailable"),
    ];
    for (index, (code, expected)) in cases.into_iter().enumerate() {
        let result = run(&session, &index.to_string(), code).await?;
        assert_ne!(result["details"]["isError"], true, "{code}: {result}");
        assert_eq!(result["content"][0]["text"], expected.trim_end(), "{code}");
    }
    for (index, code) in [
        "const print = 1",
        "throw new Error('expected failure')",
        "let invalid = ;",
    ]
    .into_iter()
    .enumerate()
    {
        let result = run(&session, &format!("error-{index}"), code).await?;
        assert_eq!(result["details"]["isError"], true, "{code}: {result}");
    }
    assert_eq!(
        run(&session, "after-error", "value").await?["content"][0]["text"],
        "42"
    );
    for (id, body, limit) in [
        ("spill", "x".repeat(100000), 2000),
        ("unicode-spill", "界🙂\n".repeat(20000), 55000),
    ] {
        let original = format!("begin\n{body}\nend\n");
        let code = format!("print({});", serde_json::to_string(original.trim_end())?);
        let spill = run(&session, id, &code).await?;
        let path = spill["details"]["meta"]["artifactId"]
            .as_str()
            .ok_or("missing spill")?;
        assert_eq!(std::fs::read_to_string(path)?, original);
        assert!(spill["content"][0]["text"]
            .as_str()
            .is_some_and(|text| text.len() < limit && text.contains("end")));
    }
    session.close().await?;
    Ok(())
}

#[tokio::test]
async fn detached_cells_queue_cancel_and_preserve_tool_ownership() -> Result {
    let (session, _root) = session(Settings {
        cell_timeout_seconds: 1,
        foreground_window_seconds: 2,
        ..Settings::default()
    })?;
    run(&session, "warm", "1").await?;
    let tools = json!([{"name":"probe","parameters":{"type":"object"}}]);
    let mut first = session.execute("first", json!({"language":"js","summary":"Wait for tool","code":"var saved = 42; (await tool.probe({slot:0})).text","timeout":1,"on_timeout":"detach"}), tools, false).await?;
    let call = event(&mut first, "call").await?;
    assert_eq!(call["parent"], "first");
    assert_eq!(call["args"]["parameters"], json!({"slot":0}));
    assert!(
        tokio::time::timeout(Duration::from_millis(1200), event(&mut first, "result"))
            .await
            .is_err(),
        "host waits must pause the one-second detach timer"
    );
    let receipt = event(&mut first, "result").await?;
    assert_eq!(
        receipt["result"]["details"]["cells"][0]["status"],
        "detached"
    );
    let mut queued = session
        .execute(
            "queued",
            json!({"language":"js","summary":"Do not run after stop","code":"saved = 0"}),
            json!([]),
            true,
        )
        .await?;
    let mut listing = session
        .execute("list", json!({"action":"list"}), json!([]), true)
        .await?;
    let listing = event(&mut listing, "result").await?;
    assert_eq!(listing["result"]["details"]["cells"][0]["cellId"], "first");
    assert!(listing["result"]["content"][0]["text"]
        .as_str()
        .is_some_and(
            |text| text.contains("queued js queued 0s queued behind first - Do not run after stop")
        ));
    let mut stopped = session
        .execute(
            "stop",
            json!({"action":"stop","cell_id":"queued"}),
            json!([]),
            true,
        )
        .await?;
    assert_eq!(
        event(&mut stopped, "result").await?["result"]["details"]["cells"][0]["status"],
        "cancelled"
    );
    assert_eq!(
        event(&mut queued, "result").await?["result"]["details"]["cells"][0]["status"],
        "cancelled"
    );
    session.send(json!({"type":"reply","id":call["id"],"result":{"content":[{"type":"text","text":"finished"}],"details":{}}})).await?;
    let settled = event(&mut first, "settled").await?;
    assert_eq!(settled["result"]["content"][0]["text"], "\"finished\"");
    assert_eq!(settled["result"]["details"]["toolCallCount"], 1);
    assert_eq!(
        run(&session, "retained", "saved").await?["content"][0]["text"],
        "42"
    );
    let mut busy = session.execute("busy", json!({"language":"js","summary":"Interrupt busy JavaScript","code":"while(true) {}","timeout":1,"on_timeout":"error"}), json!([]), false).await?;
    assert_eq!(
        event(&mut busy, "result").await?["result"]["details"]["isError"],
        true
    );
    assert_eq!(
        run(&session, "restarted", "42").await?["content"][0]["text"],
        "42"
    );
    session.close().await?;
    Ok(())
}

#[tokio::test]
async fn settled_images_and_results_obey_retention_budgets() -> Result {
    let mut settings = Settings::default();
    settings.memory.retained_images_mb = 1;
    let (session, _root) = session(settings)?;
    let images = run(&session, "image-forms", "var bytes = new Uint8Array([137,80,78,71,13,10,26,10]); for(const data of [bytes, bytes.buffer, {type:'Buffer',data:Array.from(bytes)}, Array.from(bytes).join(',')]) display({type:'image',mimeType:'image/png',data});").await?;
    assert_eq!(images["content"].as_array().map(Vec::len), Some(5));
    for frame in images["content"].as_array().into_iter().flatten().skip(1) {
        assert_eq!(frame["data"], "iVBORw0KGgo=");
    }
    let dropped = run(
        &session,
        "invalid-image",
        "display({type:'image',mimeType:'image/png',data:'abcd=efg'});",
    )
    .await?;
    assert_eq!(dropped["content"].as_array().map(Vec::len), Some(1));
    assert!(dropped["content"][0]["text"]
        .as_str()
        .is_some_and(|text| text.contains("image dropped")));
    for id in ["old-image", "new-image"] {
        let result = run(
            &session,
            id,
            "display({mimeType:'image/png',dataBase64:'a'.repeat(800000)});",
        )
        .await?;
        assert_eq!(
            result["content"][1]["data"].as_str().map(str::len),
            Some(800000)
        );
    }
    run(&session, "barrier", "1").await?;
    for (id, available) in [("old-image", false), ("new-image", true)] {
        let mut peek = session
            .execute(
                "peek",
                json!({"action":"peek","cell_id":id}),
                json!([]),
                false,
            )
            .await?;
        let result = event(&mut peek, "result").await?;
        assert_eq!(
            result["result"]["content"][1]["type"],
            if available { "image" } else { "text" }
        );
        if !available {
            assert!(result["result"]["content"][1]["text"]
                .as_str()
                .is_some_and(|text| text.contains("no longer available")));
        }
    }
    for id in 0..33 {
        run(&session, &format!("retained-{id}"), "1").await?;
    }
    let mut list = session
        .execute("list", json!({"action":"list"}), json!([]), false)
        .await?;
    let listed = event(&mut list, "result").await?;
    let cells = listed["result"]["details"]["cells"]
        .as_array()
        .ok_or("missing cells")?;
    assert!(cells.len() <= 33, "one newest result may still be settling");
    assert!(cells.iter().all(|cell| cell["cellId"] != "old-image"));
    session.close().await?;
    Ok(())
}

#[tokio::test]
async fn memory_ceiling_recycles_idle_kernel_and_announces_lost_globals() -> Result {
    let mut settings = Settings::default();
    settings.memory.gc_watermark_mb = 1;
    settings.memory.notice_mb = 1;
    settings.memory.ceiling_mb = 1;
    let (session, _root) = session(settings)?;
    let first = run(
        &session,
        "allocation",
        "var retainedBuffer = new Uint8Array(2 * 1024 * 1024); retainedBuffer.length",
    )
    .await?;
    assert_eq!(first["details"]["memory"]["gcRan"], true);
    assert_eq!(first["details"]["memory"]["overCeiling"], true);
    assert!(first["details"]["memory"]["globals"]
        .as_array()
        .is_some_and(|globals| globals
            .iter()
            .any(|entry| entry["name"] == "retainedBuffer")));
    let next = run(&session, "after-recycle", "typeof retainedBuffer").await?;
    assert_eq!(next["content"][0]["text"], "\"undefined\"");
    assert_eq!(next["details"]["memory"]["recycled"], true);
    assert!(next["content"]
        .as_array()
        .is_some_and(|parts| parts.iter().any(|part| part["text"]
            .as_str()
            .is_some_and(|text| text.contains("was restarted before this cell")))));
    session.close().await?;
    Ok(())
}

#[tokio::test]
async fn kernel_tools_are_reentrant_scoped_and_fenced_by_revision() -> Result {
    let (session, _root) = session(Settings::default())?;
    let tools = json!([{"name":"probe","parameters":{"type":"object"}}, {"name":"read","parameters":{"type":"object"}}]);
    let mut parent = session.execute("parent", json!({"language":"js","summary":"Define child tool while waiting","code":"tool(async function lookup(path) { return (await tool.read({path})).text; }, {description:'Read a path',schema:{type:'object',properties:{path:{type:'string'}},required:['path'],additionalProperties:false}}); await tool.probe({});","on_timeout":"error"}), tools.clone(), false).await?;
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
    let request = json!({"name":"lookup","kernel_generation":descriptor["kernel_generation"],"definition_revision":descriptor["definition_revision"],"args":{"path":"memo.txt"},"call_id":"child-read"});
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
    let mut parent = session.execute("parent-again", json!({"language":"js","summary":"Wait while child tools run","code":"await tool.probe({});","on_timeout":"error"}), tools.clone(), false).await?;
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
    run(&session, "recursive-definitions", "tool(async function recursive_agent() { return await agent('nested'); }); tool(async function recursive_pool() { return await workpool({category:'default',prompt:'nested'},'nested'); });").await?;
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
    let invalid = run(&session, "invalid-definition", "[() => 1, function bad(x=1){}, function* generator(){}].map(fn => {try {tool(fn); return 'accepted'} catch(error) {return error.code}})").await?;
    assert_eq!(
        invalid["content"][0]["text"],
        "[\"invalid_tool_definition\",\"invalid_tool_definition\",\"invalid_tool_definition\"]"
    );
    let revised = run(
        &session,
        "redefine",
        "tool(function lookup(path) {return path});",
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
    Ok(())
}

#[tokio::test]
async fn execution_metadata_bounds_details_without_losing_call_counts() -> Result {
    let (session, _root) = session(Settings::default())?;
    let catalog = (0..70)
        .map(|index| json!({"name":format!("probe{index}"),"parameters":{"type":"object"}}))
        .collect::<Vec<_>>();
    let mut events = session.execute("metrics", json!({"language":"js","summary":"Measure many distinct tools","code":"for(let i=0;i<110;i++) env('HARNESS_EVAL_STATUS_'+i,'ok'); await Promise.all(Array.from({length:70},(_,i)=>tool['probe'+i]({payload:'x'.repeat(1000)})))","on_timeout":"error"}), json!(catalog), false).await?;
    for _ in 0..70 {
        let call = event(&mut events, "call").await?;
        assert_eq!(
            call["args"]["parameters"]["payload"].as_str().map(str::len),
            Some(1000)
        );
        session.send(json!({"type":"reply","id":call["id"],"result":{"content":[{"type":"text","text":"ok"}],"details":{}}})).await?;
    }
    let settled = event(&mut events, "settled").await?;
    let statuses = settled["result"]["details"]["statusEvents"]
        .as_array()
        .ok_or("missing status events")?;
    assert_eq!(statuses.len(), 100);
    assert_eq!(
        statuses[0],
        json!({"op":"status-events-omitted","count":11})
    );
    assert_eq!(statuses[99]["key"], "HARNESS_EVAL_STATUS_109");
    let full = &settled["execution"];
    assert_eq!(
        settled["result"]["details"]["toolCalls"]
            .as_array()
            .map(Vec::len),
        Some(30)
    );
    assert_eq!(full["version"], 1);
    assert_eq!(full["toolCallCount"], 70);
    assert_eq!(full["pendingToolCallCount"], 0);
    assert_eq!(full["toolCalls"].as_array().map(Vec::len), Some(30));
    assert_eq!(full["toolAggregateOverflow"]["count"], 6);
    assert_eq!(full["toolCalls"][0]["argsTruncated"], true);
    assert_eq!(settled["rpcExecution"]["detailLevel"], "metadata");
    assert!(settled["rpcExecution"].to_string().len() <= 32 * 1024);
    assert!(settled["rpcExecution"]["toolCalls"]
        .as_array()
        .is_some_and(|calls| calls
            .iter()
            .all(|call| call.get("args").is_none() && call.get("resultPreview").is_none())));
    session.close().await?;
    Ok(())
}
