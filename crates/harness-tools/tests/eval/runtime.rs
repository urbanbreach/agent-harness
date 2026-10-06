use super::*;
use harness_core::{
    config::EvalConfig,
    coord::{CoordinatorHandle, RunInfo},
};
use harness_providers::{mock::MockProvider, ProviderStreamEvent as Stream};
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
#[path = "kernel_tools.rs"]
mod kernel_tools;
#[path = "performance.rs"]
mod performance;
#[path = "visual.rs"]
mod visual;
#[path = "workpools.rs"]
mod workpools;

struct Session {
    _root: tempfile::TempDir,
    handle: CoordinatorHandle,
    actor: EventActor,
    info: RunInfo,
}
impl Session {
    async fn new(
        settings: EvalConfig,
        provider: MockProvider,
        probe: Option<Arc<Probe>>,
    ) -> Result<Self> {
        Self::with_tools(
            settings,
            provider,
            probe.into_iter().map(|p| p as Arc<dyn Tool>).collect(),
            true,
        )
        .await
    }
    async fn with_tools(
        settings: EvalConfig,
        provider: MockProvider,
        tools: Vec<Arc<dyn Tool>>,
        interactive: bool,
    ) -> Result<Self> {
        signoff()?;
        let root = tempfile::tempdir()?;
        let mut registry = harness_tools::coordinator_registry(ShellAllowlist::default());
        harness_tools::register_eval_tool(&mut registry, settings);
        for tool in tools {
            registry.register(tool);
        }
        let mut config = CoordinatorConfig::new(root.path().join("sessions"));
        config.subagents.messaging_enabled = true;
        harness_tools::register_subagent_tools(
            &mut registry,
            &config.subagents,
            &Default::default(),
            None,
        );
        let mut profile = AgentProfile::fallback("default");
        profile.model_ref = "mock:eval".into();
        profile.toolset = registry.tool_ids();
        config.agent_profiles.insert("default".into(), profile);
        config
            .agent_model_targets
            .insert("default".into(), model_target());
        config
            .agent_model_targets
            .insert("smol".into(), model_target());
        config
            .agent_model_targets
            .insert("slow".into(), model_target());
        config.tool_registry = Arc::new(registry);
        config.interactive = interactive;
        config.permission_policy = PermissionPolicy::allow_all();
        config.secret_values = vec!["eval-test-credential-9f3724".into()];
        config.provider = Arc::new(provider);
        let handle = spawn_coordinator(
            config,
            Arc::new(harness_core::clock::RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );
        let info = handle.start_run("eval", root.path()).await?;
        let agent = handle
            .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
            .await?;
        Ok(Self {
            _root: root,
            handle,
            actor: EventActor::new(ActorKind::Worker, Some(agent)),
            info,
        })
    }
    async fn args(&self, args: Value) -> Result<ToolResult> {
        Ok(tokio::time::timeout(
            Duration::from_secs(15),
            self.handle
                .execute_agent_tool_call(self.actor.clone(), None, "eval", args),
        )
        .await??)
    }
    async fn run(&self, language: &str, code: &str) -> Result<ToolResult> {
        self.args(json!({"language":language,"code":code,"summary":"Verify persistent code execution","on_timeout":"error"})).await
    }
    async fn good(&self, language: &str, code: &str) -> Result<ToolResult> {
        let result = self.run(language, code).await?;
        assert!(!result.is_error(), "{language}: {}", result.display_text);
        Ok(result)
    }
}
fn settings() -> EvalConfig {
    EvalConfig {
        languages: std::env::var("HARNESS_EVAL_LANGUAGES")
            .unwrap_or_else(|_| "js,py".into())
            .split(',')
            .map(str::to_owned)
            .collect(),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn kernels_preserve_state_compose_tools_and_reset_independently() -> Result {
    let config = settings();
    let session = Session::new(
        config.clone(),
        MockProvider::default(),
        Some(Arc::new(Probe {
            started: Semaphore::new(0),
            release: [Semaphore::new(2), Semaphore::new(0)],
        })),
    )
    .await?;
    for language in &config.languages {
        let (first, second, reset, bridge) = match language.as_str() {
            "js" => ("var saved = 40; await Promise.resolve(saved + 2)", "saved += 2; saved", "typeof saved", "display(await tool_schema('read')); display(await tool.read({path:'answer.txt'})); display(await pipeline([1,2], n=>n*2, n=>n+1));"),
            "py" => ("saved = 40\nsaved + 2", "saved += 2\nsaved", "'saved' in globals()", "display(tool_schema('read'))\ndisplay(tool.read(path='answer.txt'))\ndisplay(pipeline([1,2], lambda n:n*2, lambda n:n+1))"),
            "rb" => ("$saved = 40\n$saved + 2", "$saved += 2; $saved", "defined?($saved)", "display(tool_schema('read')); display(tool.read(path: 'answer.txt'))"),
            "jl" => ("saved = 40\nsaved + 2", "saved += 2", "isdefined(Main, :saved)", "display(tool_schema(\"read\")); display(tool.read(path=\"answer.txt\"))"),
            _ => return Err("unknown signoff language".into()),
        };
        assert!(session
            .good(language, first)
            .await?
            .display_text
            .contains("42"));
        assert!(session
            .good(language, second)
            .await?
            .display_text
            .contains("42"));
        std::fs::write(
            session._root.path().join("answer.txt"),
            "bridge-through-coordinator",
        )?;
        let output = session.good(language, bridge).await?;
        assert!(
            output.display_text.contains("bridge-through-coordinator"),
            "{}",
            output.display_text
        );
        let details = output
            .structured_json
            .as_ref()
            .ok_or("missing eval details")?;
        assert!(details["runtime"]["version"]
            .as_str()
            .is_some_and(|version| !version.is_empty()));
        if matches!(language.as_str(), "js" | "py") {
            let code = if language == "js" {
                "await probe_once()"
            } else {
                "probe_once()"
            };
            assert!(session
                .good(language, code)
                .await?
                .display_text
                .contains("first"));
        }
        let reset = session.args(json!({"language":language,"code":reset,"summary":"Reset the selected kernel","reset":true,"on_timeout":"error"})).await?;
        assert!(!reset.is_error(), "{}", reset.display_text);
        assert!(!reset.display_text.contains("42"));
        let native_output = match language.as_str() {
            "js" => "import {writeSync} from 'node:fs'; writeSync(1,'native stdout\\n'); writeSync(2,'native stderr\\n'); (await import('node:child_process')).spawnSync('sh',['-c','printf child'],{stdio:'inherit'}); print(' process');",
            "py" => "import os, subprocess\nos.write(1,b'native stdout\\n')\nos.write(2,b'native stderr\\n')\nsubprocess.run(['sh','-c','printf child'],check=True)\nprint(' process')",
            "rb" => "STDOUT.write(\"native stdout\\n\"); STDOUT.flush; STDERR.write(\"native stderr\\n\"); STDERR.flush; system('sh','-c','printf child'); puts(' process');",
            "jl" => "Base.write(stdout, \"native stdout\\n\"); Base.write(stderr, \"native stderr\\n\"); flush(stdout); flush(stderr); run(`sh -c 'printf child'`); println(\" process\")",
            _ => return Err("unknown signoff language".into()),
        };
        let output = session.good(language, native_output).await?;
        assert!(
            output.display_text.contains("native stdout")
                && output.display_text.contains("native stderr")
                && output.display_text.contains("child")
                && output.display_text.contains(" process"),
            "{language}: {}",
            output.display_text
        );
    }
    let js = session.good("js", "await write('local://memo.txt', 'retained'); env('HARNESS_EVAL_LOCAL','yes'); display([await read('local://memo.txt'),env('HARNESS_EVAL_LOCAL')]); import path from 'node:path'; path.basename('/a/b')").await?;
    assert!(js.display_text.contains("retained") && js.display_text.contains("yes"));
    let snapshot = session
        .good(
            "js",
            "display([env('PI_PROVIDER'),env('PI_MODEL'),env('PI_SESSION_FILE')]);",
        )
        .await?;
    assert!(
        snapshot.display_text.contains("mock")
            && snapshot.display_text.contains("eval")
            && snapshot.display_text.contains("events.jsonl")
    );
    let local = session.args(json!({"language":"js","code":"await read('local://memo.txt')","summary":"Read the retained file after reset","reset":true,"on_timeout":"error"})).await?;
    assert!(!local.is_error() && local.display_text.contains("retained"));
    session.handle.stop_run().await?;
    let local_dir = session.info.artifacts_dir.join("eval-local").join(
        blake3::hash(
            session
                .actor
                .agent_id
                .as_deref()
                .ok_or("missing agent")?
                .as_bytes(),
        )
        .to_hex()
        .as_str(),
    );
    assert_eq!(
        std::fs::read_to_string(local_dir.join("memo.txt"))?,
        "retained"
    );
    let events = harness_core::store::read_events(&session.info.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(&e.payload, EventV1::ToolCallRequested(t) if t.tool_id == "read"))
            .count(),
        config.languages.len()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn display_images_spill_output_and_redact_durable_results() -> Result {
    use base64::Engine;
    let session = Session::new(settings(), MockProvider::default(), None).await?;
    for (name, width, format) in [
        ("small.png", 1, image::ImageFormat::Png),
        ("wide.bmp", 2200, image::ImageFormat::Bmp),
    ] {
        let pixels = image::RgbImage::from_pixel(width, 2, image::Rgb([12, 90, 120]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        pixels.write_to(&mut bytes, format)?;
        std::fs::write(session._root.path().join(name), bytes.into_inner())?;
    }
    let output = session.good("js", "display(await tool.read({path:'small.png'})); display(await (await import('node:fs/promises')).readFile('wide.bmp')); display({artifactPath:'user data', artifactId:'not a spill'}); print('start\\n' + 'x'.repeat(100000) + '\\nend'); print('eval-test-credential-9f3724')").await?;
    assert_eq!(output.attachments.len(), 2);
    for attachment in &output.attachments {
        let bytes = attachment.bytes()?;
        assert!(base64::prelude::BASE64_STANDARD.encode(&bytes).len() <= 4_718_592);
        let decoded = image::load_from_memory(&bytes)?;
        assert!(decoded.width() <= 2000 && decoded.height() <= 2000);
    }
    assert!(output.display_text.len() < 60_000);
    assert!(
        !output.artifacts.is_empty(),
        "missing overflow artifact: {}",
        output.display_text
    );
    let artifact = output
        .structured_json
        .as_ref()
        .and_then(|data| data["meta"]["artifactId"].as_str())
        .ok_or("missing spill path")?;
    assert!(std::path::Path::new(artifact).is_absolute());
    let reread = session
        .good(
            "js",
            &format!(
                "(await read({})).includes('not a spill')",
                serde_json::to_string(artifact)?
            ),
        )
        .await?;
    assert!(reread.display_text.contains("true"));
    session.handle.stop_run().await?;
    let journal = std::fs::read_to_string(&session.info.events_path)?;
    assert!(!journal.contains("eval-test-credential-9f3724"));
    for artifact in output.artifacts {
        assert!(!String::from_utf8_lossy(&std::fs::read(
            session.info.run_dir.join(artifact.path)
        )?)
        .contains("eval-test-credential-9f3724"));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn completion_and_agent_helpers_use_the_native_provider_and_task_boundaries() -> Result {
    let provider = MockProvider::script((0..8).map(|_| {
        vec![
            Stream::TextDelta("{\"answer\":42}".into()),
            Stream::DoneWithMetadata {
                usage: Some(harness_providers::CompletionUsage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    total_tokens: 15,
                }),
                metadata: Some(harness_providers::ProviderStreamFinishedMetadata {
                    settled_reasoning: Some(Vec::new()),
                    usage_complete: Some(true),
                    ..Default::default()
                }),
            },
        ]
    }));
    let session = Session::new(settings(), provider, None).await?;
    for language in ["js", "py"] {
        let code = if language == "js" {
            "display(await completion('answer', {model:'smol',schema:{type:'object'}}))"
        } else {
            "display(completion('answer', model='slow', schema={'type':'object'}))"
        };
        assert!(session
            .good(language, code)
            .await?
            .display_text
            .contains("42"));
    }
    let foreground = session
        .good(
            "js",
            "var answer = await agent('answer', {schema:{type:'object'}, tools:[]}); if(answer.answer !== 42) throw new Error('agent did not return parsed JSON'); display(answer)",
        )
        .await?;
    assert!(foreground.display_text.contains("42"));
    let background = session
        .good(
            "js",
            "var child = await agent('answer', {handle:true, tools:[]}); display(child); display(await child.control.wait({timeout:10})); display(await child.control.status()); display(await child.control.output()); try { await wait([{id:child.id,run_epoch:1}],{timeout:0}); throw new Error('stale epoch accepted'); } catch(e) { if(e.code !== 'stale_task_handle') throw e; }",
        )
        .await?;
    assert!(
        background.display_text.contains("agent://"),
        "{}",
        background.display_text
    );
    assert!(background.display_text.contains("42"));
    session.good("js", "await child.control.send('answer again'); for (const op of ['status','output','cancel']) { let rejected = false; try { await child.control[op](); } catch(e) { rejected = String(e).includes('stale task handle'); } if(!rejected) throw Error('old handle reached restarted child: '+op); } display(await wait(child.id,{timeout:10}));").await?;
    let python = session.good("py", "child = agent('answer', handle=True, tools=[])\ndisplay(child.control.wait(timeout=10))\ndisplay(child.control.status())\ndisplay(child.control.output())").await?;
    assert!(python.display_text.contains("42"));
    let output = session
        .good("js", "display(await output(child.id))")
        .await?;
    assert!(
        !output.display_text.contains("TaskNotFound"),
        "{}",
        output.display_text
    );
    session.handle.stop_run().await?;
    let events = harness_core::store::read_events(&session.info.events_path)?;
    assert_eq!(events.iter().filter(|e| matches!(&e.payload, EventV1::ProviderRequestStarted(t) if t.prompt_summary == "Eval completion")).count(),2);
    Ok(())
}

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn detached_and_queued_cells_keep_ownership_until_one_terminal_notification() -> Result {
    let probe = Arc::new(Probe {
        started: Semaphore::new(0),
        release: [Semaphore::new(0), Semaphore::new(0)],
    });
    let config = EvalConfig {
        cell_timeout_seconds: 1,
        foreground_window_seconds: 1,
        ..settings()
    };
    let session = Session::new(config, MockProvider::default(), Some(Arc::clone(&probe))).await?;
    let mut events = session.handle.subscribe_new_events().await?;
    let receipt = session.args(json!({"language":"js","summary":"Wait for a native tool","code":"var kept = 42; await tool.probe({slot:0}); kept"})).await?;
    assert_eq!(
        receipt.structured_json.as_ref().ok_or("missing receipt")?["detached"],
        true
    );
    let id = receipt.structured_json.as_ref().ok_or("missing receipt")?["cell_id"]
        .as_str()
        .ok_or("missing cell id")?
        .to_owned();
    let peek = session.args(json!({"action":"peek","cell_id":id})).await?;
    assert!(!peek.is_error());
    assert_eq!(
        peek.structured_json
            .as_ref()
            .ok_or("missing peek details")?["cell_id"],
        id
    );
    let reset = session
        .args(json!({"language":"js","code":"1","summary":"Refuse a busy reset","reset":true}))
        .await?;
    assert!(reset.is_error(), "{}", reset.display_text);
    let queued = session
        .args(json!({"language":"js","code":"kept + 1","summary":"Queue another cell"}))
        .await?;
    let queued_id = queued
        .structured_json
        .as_ref()
        .ok_or("missing queued receipt")?["cell_id"]
        .as_str()
        .ok_or("missing queued id")?
        .to_owned();
    let stopped = session
        .args(json!({"action":"stop","cell_id":queued_id}))
        .await?;
    assert!(!stopped.is_error(), "{}", stopped.display_text);
    probe.release[0].add_permits(1);
    tokio::time::timeout(Duration::from_secs(10),async {
        while let Some(event) = events.next().await {
            if matches!(event?.payload,EventV1::EvalCellFinished(ref t) if t.tool_call_id.as_str()==id) { return Ok::<_,Box<dyn std::error::Error>>(()); }
        }
        Err("detached cell never completed".into())
    }).await??;
    assert!(session
        .good("js", "kept")
        .await?
        .display_text
        .contains("42"));
    session.handle.stop_run().await?;
    let events = harness_core::store::read_events(&session.info.events_path)?;
    assert_eq!(
        events
            .iter()
            .filter(
                |e| matches!(&e.payload,EventV1::ToolCallFinished(t) if t.tool_call_id.as_str()==id)
            )
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(
                |e| matches!(&e.payload,EventV1::EvalCellFinished(t) if t.tool_call_id.as_str()==id)
            )
            .count(),
        1
    );
    assert_eq!(events.iter().filter(|e| matches!(&e.payload,EventV1::UserMessageSubmitted(t) if t.text.starts_with(&format!("Eval cell {id} completed.")))).count(),1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires local eval runtimes; scripts/test-lanes.sh eval"]
async fn execution_deadlines_interrupt_busy_kernels_and_allow_the_next_cell() -> Result {
    let config = EvalConfig {
        cell_timeout_seconds: 1,
        foreground_window_seconds: 1,
        ..settings()
    };
    let session =
        Session::with_tools(config.clone(), MockProvider::default(), Vec::new(), false).await?;
    for (language, code) in [
        ("js", "while(true) {}"),
        ("py", "kept = 42\nwhile True: pass"),
        ("rb", "$kept = 42; loop {}"),
        ("jl", "kept = 42; while true; end"),
    ] {
        if !config.languages.iter().any(|enabled| enabled == language) {
            continue;
        }
        let result = session.args(json!({"language":language,"code":code,"summary":"Interrupt a busy kernel","timeout":1,"on_timeout":"error"})).await?;
        assert!(result.is_error(), "{}", result.display_text);
        let code = if language == "py" { "kept" } else { "42" };
        assert!(session
            .good(language, code)
            .await?
            .display_text
            .contains("42"));
    }
    let output = session.args(json!({"language":"js","summary":"Wait in a noninteractive session","code":"await new Promise(resolve => setTimeout(resolve, 1100)); 42"})).await?;
    assert!(!output.is_error() && output.display_text.contains("42"));
    assert_ne!(
        output.structured_json.as_ref().ok_or("missing result")?["detached"],
        true
    );
    session.handle.stop_run().await?;
    Ok(())
}
