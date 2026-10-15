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
    client.call("session.intent",json!({"sessionId":id,"intent":{"kind":"submit","text":"Describe a copper lighthouse"}})).await?;
    loop {
        if client.events.iter().any(|message| {
            message["params"]["update"]["event"]["payload"]["event_type"]
                == "assistant_message_finished"
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
