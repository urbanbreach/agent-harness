use super::*;
use tokio::io::{DuplexStream, ReadHalf, WriteHalf};

struct Client {
    reader: tokio::io::Lines<BufReader<ReadHalf<DuplexStream>>>,
    writer: WriteHalf<DuplexStream>,
    id: u64,
    events: Vec<Value>,
}
impl Client {
    async fn next(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let line =
            tokio::time::timeout(std::time::Duration::from_secs(20), self.reader.next_line())
                .await??
                .ok_or("server closed")?;
        Ok(serde_json::from_str(&line)?)
    }
    async fn call(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        self.id += 1;
        write_message(
            &mut self.writer,
            &json!({"id":self.id,"method":method,"params":params}),
        )
        .await?;
        loop {
            let message = self.next().await?;
            if message.get("id").and_then(Value::as_u64) == Some(self.id) {
                if message.get("error").is_some() {
                    return Err(format!("request failed: {message}").into());
                }
                return Ok(message["result"].clone());
            }
            self.events.push(message);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gui_server_round_trip_persists_and_resumes_conversation(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let cwd = root.path().join("project");
    std::fs::create_dir(&cwd)?;
    let context = harness::CliDeps::real()
        .with_current_dir(cwd.clone())
        .with_env("HARNESS_HOME", home.to_string_lossy())
        .config_load_context();
    let (client, server) = tokio::io::duplex(1024 * 1024);
    let (reader, writer) = tokio::io::split(server);
    let server = tokio::spawn(serve(reader, writer, true, None, None, None, context));
    let (reader, writer) = tokio::io::split(client);
    let mut client = Client {
        reader: BufReader::new(reader).lines(),
        writer,
        id: 0,
        events: Vec::new(),
    };
    let initialized = client.call("app.initialize", json!({})).await?;
    assert_eq!(initialized["home"], home.to_string_lossy().as_ref());
    assert!(
        !home.exists(),
        "initialization must not create home artifacts"
    );
    std::fs::create_dir_all(cwd.join(".harness/commands"))?;
    std::fs::write(
        cwd.join(".harness/commands/review.md"),
        "Review $1 against $2. Full: $ARGUMENTS",
    )?;
    assert_eq!(
        client
            .call(
                "commands.expand",
                json!({
                    "cwd":cwd,"name":"review","args":"\"two words\" base"
                })
            )
            .await?["text"],
        "Review two words against base. Full: \"two words\" base"
    );
    assert!(client
        .call(
            "commands.expand",
            json!({"cwd":cwd,"name":"missing","args":""})
        )
        .await
        .is_err());
    assert_eq!(
        client.call("config.validate", json!({"cwd":cwd})).await?["valid"],
        true
    );
    let protocols = client
        .call("providers.protocols", json!({"cwd":cwd}))
        .await?;
    assert!(protocols
        .as_array()
        .ok_or("protocols must be an array")?
        .iter()
        .any(|protocol| protocol["protocol"] == "anthropic_messages"));
    assert!(client
        .call("worktrees.list", json!({"cwd":cwd,"all":"yes"}))
        .await
        .is_err());
    assert_eq!(
        client
            .call("state.read", json!({"name":"workspace"}))
            .await?,
        Value::Null
    );
    client
        .call(
            "state.write",
            json!({"name":"workspace","value":{"sidebar":false}}),
        )
        .await?;
    assert_eq!(
        client
            .call("state.read", json!({"name":"workspace"}))
            .await?,
        json!({"sidebar":false})
    );
    client
        .call(
            "settings.write",
            json!({"cwd":cwd,"id":"subagents.max_depth","value":2}),
        )
        .await?;
    let registry = client.call("settings.list", json!({"cwd":cwd})).await?;
    assert_eq!(
        registry
            .as_array()
            .ok_or("registry must be an array")?
            .iter()
            .find(|row| row["id"] == "subagents.max_depth")
            .ok_or("missing subagent depth setting")?["value"],
        json!(2)
    );
    client
        .call(
            "settings.write",
            json!({"cwd":cwd,"id":"runtime.compaction.enabled","value":false}),
        )
        .await?;
    assert_eq!(
        harness_core::config::read_project_setting_value(
            &cwd.join("harness.jsonc"),
            "runtime.compaction.enabled"
        )?,
        Some("false".into())
    );
    client
        .call(
            "settings.reset",
            json!({"cwd":cwd,"id":"runtime.compaction.enabled"}),
        )
        .await?;
    assert_eq!(
        harness_core::config::read_project_setting_value(
            &cwd.join("harness.jsonc"),
            "runtime.compaction.enabled"
        )?,
        None
    );
    let session = client.call("session.create", json!({"cwd":cwd})).await?;
    let id = session["sessionId"].clone();
    let run_dir = session["runDir"].clone();
    for mode in ["autoEdits", "auto", "full", "supervised"] {
        client
            .call(
                "session.intent",
                json!({"sessionId":id,"intent":{"kind":"setAccessMode","mode":mode}}),
            )
            .await?;
        assert_eq!(
            client.call("session.info", json!({"sessionId":id})).await?["accessMode"],
            mode
        );
    }
    client.call("session.intent",json!({"sessionId":id,"intent":{"kind":"submit","text":"Describe a copper lighthouse"}})).await?;
    loop {
        if client.events.iter().any(|message| {
            message["params"]["update"]["event"]["payload"]["event_type"]
                == "assistant_message_finished"
        }) && client.events.iter().any(|message| {
            message["params"]["update"]["event"]["payload"]["event_type"] == "task_completed"
        }) {
            break;
        }
        let message = client.next().await?;
        client.events.push(message);
    }
    let events = client
        .call("sessions.events", json!({"runDir":run_dir}))
        .await?;
    assert!(events
        .as_array()
        .ok_or("events must be an array")?
        .iter()
        .any(|event| event["payload"]["event_type"] == "user_message_submitted"));
    let source_events = events.as_array().ok_or("events must be an array")?;
    let started = source_events
        .iter()
        .find(|event| event["payload"]["event_type"] == "run_started")
        .ok_or("missing run start")?;
    assert_eq!(started["payload"]["data"]["workspace_root"], json!(cwd));
    let prompt = source_events
        .iter()
        .find(|event| event["payload"]["event_type"] == "user_message_submitted")
        .ok_or("missing source prompt")?;
    client
        .call(
            "session.intent",
            json!({"sessionId":id,"intent":{"kind":"fork","promptRequestId":prompt["payload"]["data"]["request_id"]}}),
        )
        .await?;
    let forked = loop {
        if let Some(message) = client
            .events
            .iter()
            .find(|message| message["params"]["update"]["kind"] == "forked")
        {
            break message["params"]["update"].clone();
        }
        let message = client.next().await?;
        client.events.push(message);
    };
    let fork_id = &forked["runId"];
    let fork_dir = &forked["runDir"];
    let catalog = client.call("sessions.list", json!({"cwd":cwd})).await?;
    let child = catalog
        .as_array()
        .ok_or("sessions must be an array")?
        .iter()
        .find(|row| row["runId"] == *fork_id)
        .ok_or("fork missing from project catalog")?;
    assert_eq!(child["cwd"], json!(cwd));
    assert_eq!(child["catalog"]["parent_session_id"], id);
    assert_eq!(child["catalog"]["is_resumable"], json!(true));
    let fork_events = client
        .call("sessions.events", json!({"runDir":fork_dir}))
        .await?;
    assert!(fork_events
        .as_array()
        .ok_or("fork events must be an array")?
        .iter()
        .any(|event| event["payload"]["event_type"] == "assistant_message_finished"));
    let continued = client
        .call("session.continue", json!({"runDir":fork_dir}))
        .await?;
    assert_eq!(continued["cwd"], json!(cwd));
    client.call("session.intent", json!({"sessionId":fork_id,"intent":{"kind":"submit","text":"Continue the forked conversation"}})).await?;
    loop {
        let message = client.next().await?;
        let is_prompt = message["params"]["sessionId"] == *fork_id
            && message["params"]["update"]["event"]["payload"]["event_type"]
                == "user_message_submitted";
        client.events.push(message);
        if is_prompt {
            break;
        }
    }
    client
        .call("session.close", json!({"sessionId":fork_id}))
        .await?;
    client
        .call("sessions.delete", json!({"runDir":fork_dir}))
        .await?;
    client
        .call("session.close", json!({"sessionId":id}))
        .await?;
    client
        .call(
            "sessions.rename",
            json!({"runDir":run_dir,"title":"Copper lighthouse"}),
        )
        .await?;
    let rows = client.call("sessions.list", json!({"cwd":cwd})).await?;
    assert_eq!(rows.as_array().ok_or("sessions must be an array")?.len(), 1);
    assert_eq!(rows[0]["projectCwd"], json!(cwd));
    let rebuilt = client
        .call("diagnostics.rebuildSessionIndex", json!({"cwd":cwd}))
        .await?;
    assert_eq!(rebuilt["entry_count"], 1);
    assert!(Path::new(rebuilt["index_path"].as_str().ok_or("missing index path")?).is_file());
    let scan = client
        .call("diagnostics.crashScan", json!({"cwd":cwd}))
        .await?;
    let reports = scan["reports"]
        .as_array()
        .ok_or("crash reports must be an array")?;
    assert!(reports
        .iter()
        .any(|report| report["run_dir"] == run_dir && report["events_log_present"] == true));
    let explanation = client
        .call(
            "config.explain",
            json!({"cwd":cwd,"path":"subagents.max_depth"}),
        )
        .await?;
    assert_eq!(explanation["effective"], 2);
    let matches = client
        .call(
            "sessions.search",
            json!({"cwds":[cwd],"query":"copper lighthouse","limit":10}),
        )
        .await?;
    assert!(!matches
        .as_array()
        .ok_or("search must be an array")?
        .is_empty());
    let left_live = client.call("session.create", json!({"cwd":cwd})).await?;
    let resumed = client
        .call("session.continue", json!({"runDir":run_dir}))
        .await?;
    assert_eq!(resumed["sessionId"], id);
    client
        .call("session.close", json!({"sessionId":id}))
        .await?;
    let trashed = client
        .call("sessions.delete", json!({"runDir":run_dir}))
        .await?;
    assert!(Path::new(trashed["trashedTo"].as_str().ok_or("missing trash path")?).exists());
    client.writer.shutdown().await?;
    server.await??;
    let stopped = crate::cli_io::load_events_from_run_dir(Path::new(
        left_live["runDir"]
            .as_str()
            .ok_or("missing run directory")?,
    ))?;
    assert!(stopped
        .iter()
        .any(|event| matches!(event.payload, EventV1::RunFinished(_))));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scripted_mock_completion_is_tool_free_without_provider_configuration(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let script_path = root.path().join("script.json");
    std::fs::write(
        &script_path,
        r#"[[{"kind":"text","text":"Describe the change"},{"kind":"stop"}],[{"kind":"tool","id":"eval-check","toolId":"eval","args":{"language":"js","summary":"Check native evaluation","code":"display('native evaluation ran')"}},{"kind":"stop"}],[{"kind":"text","text":"Evaluation finished"},{"kind":"stop"}]]"#,
    )?;
    let context = harness::CliDeps::real()
        .with_current_dir(root.path().to_owned())
        .with_env("HARNESS_HOME", home.to_string_lossy())
        .config_load_context();
    let (client, server) = tokio::io::duplex(1024 * 1024);
    let (reader, writer) = tokio::io::split(server);
    let server = tokio::spawn(serve(
        reader,
        writer,
        true,
        Some(mock_script::Script::read(&script_path)?),
        None,
        None,
        context,
    ));
    let (reader, writer) = tokio::io::split(client);
    let mut client = Client {
        reader: BufReader::new(reader).lines(),
        writer,
        id: 0,
        events: Vec::new(),
    };
    let result = client
        .call(
            "llm.complete",
            json!({
                "cwd":root.path(),"prompt":"Write a commit message"
            }),
        )
        .await?;
    assert_eq!(result["text"], "Describe the change");
    assert_eq!(result["provider"], "mock");
    assert!(
        !home.exists(),
        "completion must not create sessions or credentials"
    );
    assert!(client
        .call("llm.complete", json!({"cwd":root.path(),"prompt":" "}))
        .await
        .is_err());
    let session = client
        .call(
            "session.create",
            json!({"cwd":root.path(),"accessMode":"full"}),
        )
        .await?;
    let id = &session["sessionId"];
    client
        .call(
            "session.intent",
            json!({"sessionId":id,"intent":{"kind":"submit","text":"First scripted turn"}}),
        )
        .await?;
    loop {
        let message = client.next().await?;
        if message["params"]["update"]["event"]["payload"]["event_type"] == "task_completed" {
            break;
        }
    }
    client
        .call(
            "session.intent",
            json!({"sessionId":id,"intent":{"kind":"submit","text":"Execute the evaluation"}}),
        )
        .await?;
    let mut eval_call = None;
    loop {
        let message = client.next().await?;
        let payload = &message["params"]["update"]["event"]["payload"];
        if payload["event_type"] == "tool_call_requested" {
            assert_eq!(payload["data"]["tool_id"], "eval");
            eval_call = Some(payload["data"]["tool_call_id"].clone());
        }
        if payload["event_type"] == "tool_call_finished" {
            assert_eq!(Some(payload["data"]["tool_call_id"].clone()), eval_call);
            assert_eq!(payload["data"]["status"], "succeeded");
            break;
        }
    }
    client.writer.shutdown().await?;
    server.await??;
    Ok(())
}
