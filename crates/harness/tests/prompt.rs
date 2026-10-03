use harness::{run, CliDeps, CliIo};
use harness_core::{event::EventV1, store::read_events};
use harness_providers::{mock::MockProvider, Provider, ProviderStreamEvent};
use std::{fs, io::Cursor, sync::Arc};

#[tokio::test]
async fn prompt_resume_and_fork_restore_history_and_export_only_committed_events(
) -> Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let elsewhere = tempfile::tempdir()?;
    let sessions = workspace.path().join("sessions");
    let input_file = workspace.path().join("prompt.txt");
    fs::write(&input_file, "First prompt\n")?;
    let config = workspace.path().join("credentials.json");
    fs::write(&config, serde_json::json!({
        "provider":{"fixture":{"type":"openai_compatible","apiKey":"formerly-private-token","models":{"fixture":{}}}}
    }).to_string())?;
    let provider = Arc::new(MockProvider::script(
        [
            "First answer formerly-private-token",
            "Second answer",
            "Fork answer",
        ]
        .map(|text| {
            vec![
                ProviderStreamEvent::TextDelta(text.into()),
                ProviderStreamEvent::Done { usage: None },
            ]
        }),
    ));
    let source = sessions.join("chosen-session");
    let mut original = Vec::new();
    for step in 0..3 {
        let mut args = vec![
            "harness",
            "--session-dir",
            sessions.to_str().ok_or("sessions path")?,
        ];
        if step > 0 {
            args.extend(["--config", config.to_str().ok_or("config path")?]);
        }
        args.extend(["prompt", "--mock"]);
        let mut format = "default";
        match step {
            0 => args.extend([
                "--prompt-file",
                input_file.to_str().ok_or("prompt path")?,
                "--session-id",
                "chosen-session",
            ]),
            1 => {
                args.extend(["--resume", "chosen-session", "--text", "Second prompt"]);
                format = "json";
            }
            _ => {
                args.extend([
                    "--resume",
                    source.to_str().ok_or("source path")?,
                    "--fork-session",
                    "--session-id",
                    "chosen-fork",
                    "--text",
                    "Fork prompt",
                ]);
                format = "streaming-json";
            }
        }
        let export = workspace.path().join(format!("export-{step}.jsonl"));
        args.extend([
            "--format",
            format,
            "--out",
            export.to_str().ok_or("export path")?,
        ]);
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            args,
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(
                    if step == 0 {
                        workspace.path()
                    } else {
                        elsewhere.path()
                    }
                    .into(),
                )
                .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let run_dir = if step == 2 {
            sessions.join("chosen-fork")
        } else {
            source.clone()
        };
        let events = read_events(&run_dir.join("events.jsonl"))?;
        assert!(matches!(
            events.last().map(|e| &e.payload),
            Some(EventV1::RunFinished(_))
        ));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, EventV1::AgentSpawned(_)))
                .count(),
            1
        );
        assert_eq!(read_events(&export)?.len(), events.len());
        if step == 0 {
            assert_eq!(read_events(&export)?, events);
            assert_eq!(
                String::from_utf8(stdout)?.trim(),
                "First answer formerly-private-token"
            );
        } else {
            assert!(
                !fs::read_to_string(&export)?.contains("formerly-private-token"),
                "export must redact credentials learned after the original run"
            );
            assert!(
                fs::read_to_string(source.join("events.jsonl"))?.contains("formerly-private-token"),
                "export cannot rewrite the source journal"
            );
            let output: Vec<harness_core::event::RuntimeEvent> = if step == 1 {
                serde_json::from_slice(&stdout)?
            } else {
                String::from_utf8(stdout)?
                    .lines()
                    .map(serde_json::from_str)
                    .collect::<Result<_, _>>()?
            };
            assert!(output.iter().any(|event| matches!(event,
                harness_core::event::RuntimeEvent::Durable(event) if matches!(event.payload, EventV1::AssistantMessageFinished(_)))));
            let output = serde_json::to_string(&output)?;
            assert!(
                !output.contains("First answer"),
                "resume must not replay old output"
            );
        }
        let request = provider
            .captured_requests()
            .await
            .pop()
            .ok_or("provider request")?;
        let user_text: Vec<_> = request
            .messages
            .iter()
            .filter(|m| m.role == harness_providers::MessageRole::User)
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(
            user_text,
            ["First prompt\n", "Second prompt", "Fork prompt"][..=step]
        );
        assert!(events
            .iter()
            .filter_map(|e| match &e.payload {
                EventV1::RunStarted(e) => Some(&e.workspace_root),
                _ => None,
            })
            .all(|path| std::path::Path::new(path) == workspace.path()));
        if step < 2 {
            original = fs::read(source.join("events.jsonl"))?;
        } else {
            assert_eq!(fs::read(source.join("events.jsonl"))?, original);
        }
    }
    assert_eq!(provider.call_count(), 3);
    Ok(())
}

#[tokio::test]
async fn prompt_options_select_model_tools_and_policy_before_execution(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let config = root.path().join("config.json");
    fs::write(&config, serde_json::json!({
        "provider":{"local":{"type":"openai_compatible", "models":{"base":{}, "chosen":{"variants":{"focused":{"metadata":{"reasoning_effort":"high"}}}}}}},
        "model":"local/base", "agent":{"custom":{"tools":["write","read","list","task","spawn_subagent","webfetch","websearch"],"system_prompt":"Original instructions","permission":{"read":"deny"}}},
        "permission":{"edit":{"protected.txt":"deny"}}
    }).to_string())?;
    let provider = Arc::new(MockProvider::script([
        vec![
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "allowed".into(),
                function_name: "write".into(),
                arguments_json: serde_json::json!({"path":"allowed.txt","content":"ok"})
                    .to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "denied".into(),
                function_name: "write".into(),
                arguments_json: serde_json::json!({"path":"protected.txt","content":"no"})
                    .to_string(),
            },
            ProviderStreamEvent::Done { usage: None },
        ],
        vec![
            ProviderStreamEvent::TextDelta("Options applied".into()),
            ProviderStreamEvent::Done { usage: None },
        ],
    ]));
    let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
    let result = run(
        [
            "harness",
            "--config",
            config.to_str().ok_or("config path")?,
            "prompt",
            "--text",
            "Do the work",
            "--profile",
            "custom",
            "--model",
            "local/chosen",
            "--variant",
            "focused",
            "--reasoning-effort",
            "low",
            "--thinking",
            "--max-turns",
            "2",
            "--tools",
            "write,read,list,task,spawn_subagent,webfetch,websearch",
            "--disallowed-tools",
            "read",
            "--no-subagents",
            "--disable-web-search",
            "--no-memory",
            "--system-prompt-override",
            "Override instructions",
            "--rules",
            "Extra rule",
            "--permission-mode",
            "dontAsk",
            "--allow",
            "edit",
        ],
        &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
        CliDeps::real()
            .with_current_dir(root.path().into())
            .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
    );
    assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert!(root.path().join("allowed.txt").is_file());
    assert!(
        !root.path().join("protected.txt").exists(),
        "command defaults cannot bypass an explicit deny rule"
    );
    let requests = provider.captured_requests().await;
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request.model_id, "chosen");
        assert_eq!(request.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(request.reasoning_summary.as_deref(), Some("auto"));
        assert_eq!(
            request.messages[0].content,
            "Override instructions\n\nExtra rule"
        );
        assert_eq!(
            request
                .tools
                .ok_or("missing tool definitions")?
                .iter()
                .map(|t| t.tool_id.as_str())
                .collect::<Vec<_>>(),
            ["write"]
        );
    }
    Ok(())
}

#[tokio::test]
async fn prompt_resolves_subagent_enablement_and_messaging_before_provider_dispatch(
) -> Result<(), Box<dyn std::error::Error>> {
    for (enabled, messaging) in [(false, true), (true, false)] {
        let root = tempfile::tempdir()?;
        let config = root.path().join("config.json");
        fs::write(
            &config,
            serde_json::json!({
                "subagents": {"enabled": enabled},
                "features": {"active_agent_messages": messaging},
                "agent": {"custom": {"tools": [
                    "spawn_subagent", "send_subagent_message",
                    "get_command_or_subagent_output"
                ]}}
            })
            .to_string(),
        )?;
        let provider = Arc::new(MockProvider::script([vec![ProviderStreamEvent::Done {
            usage: None,
        }]]));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                config.to_str().ok_or("config path")?,
                "prompt",
                "--mock",
                "--profile",
                "custom",
                "--text",
                "Inspect configured tools",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .without_env("GROK_SUBAGENTS")
                .without_env("GROK_ACTIVE_AGENT_MESSAGES")
                .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let request = provider
            .captured_requests()
            .await
            .pop()
            .ok_or("provider request")?;
        let tools = request.tools.ok_or("missing tool definitions")?;
        assert_eq!(
            tools.iter().any(|tool| tool.tool_id == "spawn_subagent"),
            enabled
        );
        assert_eq!(
            tools
                .iter()
                .any(|tool| tool.tool_id == "send_subagent_message"),
            messaging
        );
        assert!(tools
            .iter()
            .any(|tool| tool.tool_id == "get_command_or_subagent_output"));
    }
    Ok(())
}

#[tokio::test]
async fn prompt_uses_the_runtime_catalog_with_environment_or_stored_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::{
        auth::{CredentialStore, ProviderId, StoredCredential},
        provider_catalog::ProviderCatalog,
    };
    let catalog = ProviderCatalog::from_embedded()?;
    for source in [
        "environment",
        "stored",
        "codex",
        "codex-config",
        "codex-cli",
    ] {
        let root = tempfile::tempdir()?;
        let data = root.path().join("data");
        let config = root.path().join("configuration");
        std::fs::create_dir(&config)?;
        let provider = Arc::new(MockProvider::script([vec![
            ProviderStreamEvent::TextDelta("Catalog selected. opaque-catalog-credential".into()),
            ProviderStreamEvent::Done { usage: None },
        ]]));
        let mut deps = CliDeps::real()
            .with_current_dir(root.path().into())
            .with_env("HARNESS_DATA_HOME", data.to_str().ok_or("data path")?)
            .with_env("XDG_CONFIG_HOME", config.to_str().ok_or("config path")?)
            .without_env("HARNESS_CONFIG")
            .without_env("HARNESS_CONFIG_CONTENT")
            .with_provider_override(Arc::clone(&provider) as Arc<dyn Provider>);
        for entry in catalog.sorted_by_priority() {
            for name in &entry.api_key_env {
                deps = deps.without_env(name);
            }
        }
        if source == "environment" {
            deps = deps.with_env("OPENAI_API_KEY", "opaque-catalog-credential");
        } else if source == "codex-cli" {
            let (mut input, mut stdout, mut stderr) =
                (Cursor::new(Vec::new()), Vec::new(), Vec::new());
            let login = run(
                [
                    "harness",
                    "auth",
                    "login",
                    "openai-codex",
                    "--mock-token",
                    "opaque-catalog-credential",
                ],
                &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
                deps.clone(),
            );
            assert_eq!(login.code, 0, "{}", String::from_utf8_lossy(&stderr));
        } else if source.starts_with("codex") {
            CredentialStore::new(data.join("harness")).save(&StoredCredential::oauth(
                ProviderId::codex(),
                "opaque-catalog-credential",
                "opaque-refresh-credential",
                None,
                "2026-09-26T00:00:00Z",
            ))?;
            if source == "codex-config" {
                let path = root.path().join("fixture.json");
                std::fs::write(
                    &path,
                    r#"{"agent":{"default":{"system_prompt":"Keep these instructions.","tools":[]}}}"#,
                )?;
                deps = deps.with_env("HARNESS_CONFIG", path.to_str().ok_or("fixture path")?);
            }
        } else {
            CredentialStore::new(data.join("harness")).save(&StoredCredential::api_key(
                ProviderId::parse("openai").ok_or("provider id")?,
                "opaque-catalog-credential",
                "2026-09-26T00:00:00Z",
            ))?;
        }
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            ["harness", "prompt", "--text", "Use my connected provider"],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            deps,
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let requests = provider.captured_requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].provider_id.as_deref(),
            Some(if source.starts_with("codex") {
                "openai-codex"
            } else {
                "openai"
            })
        );
        if source.starts_with("codex") {
            assert_eq!(requests[0].model_id, "gpt-6-astra");
        }
        if source == "codex-config" {
            assert!(requests[0].messages[0]
                .content
                .contains("Keep these instructions."));
        }
        assert_eq!(
            String::from_utf8(stdout)?.trim(),
            "Catalog selected. [REDACTED]"
        );
    }
    Ok(())
}
