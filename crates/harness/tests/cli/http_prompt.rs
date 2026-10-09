use super::*;
use harness_providers::{mock::MockProvider, ProviderStreamEvent};
use std::{
    io::{BufRead, Read},
    net::TcpListener,
    time::Duration,
};

#[test]
fn configured_http_prompt_runs_tools_under_coordinator_policy(
) -> Result<(), Box<dyn std::error::Error>> {
    for permission in ["allow", "deny", "stored"] {
        let root = tempfile::tempdir()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let endpoint = format!(
            "http://{}/v1{}",
            listener.local_addr()?,
            if permission == "stored" {
                "/responses/?trace=opaque%2Dquery%2Dtoken"
            } else {
                ""
            }
        );
        let config = root.path().join("runtime.json");
        fs::write(&config, json!({
            "provider":{"local":{"type":"openai_compatible", "baseURL":endpoint, "apiMode": if permission == "stored" {"auto"} else {"chat_completions"},
                "apiKey":if permission == "allow" {"${HARNESS_TEST_KEY}"} else {""},
                "apiKeyEnv":if permission == "deny" {vec!["HARNESS_TEST_KEY"]} else {Vec::<&str>::new()},
                "models":{"fixture":{}}}},
            "model":"local/fixture", "agent":{"default":{"tools":["write"],"system_prompt":(permission == "allow").then_some("Write only when permitted.")}},
            "instructions": "Follow the project instructions.",
            "permission":{"edit":if permission == "stored" {"allow"} else {permission}}, "runtime":{"provider_retry":{"max_retries":0}}
        }).to_string())?;
        let mut deps = CliDeps::real()
            .with_current_dir(root.path().into())
            .without_env("HOME")
            .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy())
            .without_env("LOCALAPPDATA")
            .without_env("APPDATA")
            .with_env("HARNESS_TEST_KEY", "private-fixture-token");
        if permission == "stored" {
            let data = root.path().join("data");
            harness_core::auth::CredentialStore::new(data.clone()).save(
                &harness_core::auth::StoredCredential::api_key(
                    harness_core::auth::ProviderId::parse("local").ok_or("provider id")?,
                    "private-fixture-token",
                    "2026-09-26T00:00:00Z",
                ),
            )?;
            deps = deps.with_env("HARNESS_HOME", data.to_str().ok_or("data path")?);
        }
        let server = std::thread::spawn(move || serve_provider(listener, permission == "stored"));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                "runtime.json",
                "prompt",
                "--text",
                "Write answer.txt. The request mentions private-fixture-token.",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let requests = server.join().map_err(|_| "HTTP fixture stopped")??;
        let system = requests[0]["messages"][0]["content"]
            .as_str()
            .ok_or("system prompt missing")?;
        assert!(system.contains("Follow the project instructions."));
        assert_eq!(
            system.contains("Write only when permitted."),
            permission == "allow"
        );
        assert_eq!(
            system.contains("You are Harness, a coding agent."),
            permission != "allow"
        );
        assert_eq!(requests[0]["model"], "fixture");
        assert_eq!(
            requests[0]["tools"][0]["function"]["name"].as_str(),
            (permission != "deny").then_some("write")
        );
        assert_eq!(
            requests[1]["messages"]
                .as_array()
                .ok_or("messages missing")?
                .last()
                .ok_or("tool result missing")?["role"],
            "tool"
        );
        assert_eq!(
            root.path().join("answer.txt").exists(),
            permission != "deny"
        );
        check_attribution(root.path(), &deps, permission)?;
        assert_eq!(
            String::from_utf8(stdout)?.trim(),
            if permission == "stored" {
                "Finished after the tool result. [REDACTED] [REDACTED]"
            } else {
                "Finished after the tool result. [REDACTED]"
            }
        );
        let sessions =
            harness_core::storage_paths::ProjectPaths::new(&root.path().join("data"), root.path())?
                .sessions_dir();
        let run = fs::read_dir(sessions)?
            .next()
            .ok_or("session missing")??
            .path();
        assert!(!fs::read_to_string(run.join("events.jsonl"))?.contains("private-fixture-token"));
        assert!(!fs::read_to_string(run.join("events.jsonl"))?.contains("opaque-query-token"));
    }
    Ok(())
}

fn check_attribution(
    root: &std::path::Path,
    deps: &CliDeps,
    permission: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if permission != "stored" {
        return Ok(());
    }
    fs::write(
        root.join("answer.txt"),
        "created\nprivate-fixture-token opaque-query-token\n",
    )?;
    for action in ["diff", "blame"] {
        let (mut input, mut report, mut errors) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                "runtime.json",
                "attribution",
                action,
                "answer.txt",
            ],
            &mut CliIo::new(&mut input, &mut report, &mut errors),
            deps.clone(),
        );
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&errors));
        let parsed: serde_json::Value = serde_json::from_slice(&report)?;
        assert_eq!(parsed["drifted"], true);
        let text = String::from_utf8(report)?;
        assert!(!text.contains("private-fixture-token"));
        assert!(!text.contains("opaque-query-token"));
        assert!(text.contains("[REDACTED]"));
    }
    Ok(())
}

#[tokio::test]
async fn prompt_commits_before_completion_and_replay_never_calls_the_provider(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let sessions = root.path().join("sessions");
    let provider = Arc::new(MockProvider::script([vec![
        ProviderStreamEvent::TextDelta("A durable answer.".into()),
        ProviderStreamEvent::Done { usage: None },
    ]]));
    let deps = CliDeps::real()
        .with_current_dir(root.path().into())
        .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy())
        .with_provider_override(Arc::clone(&provider) as Arc<dyn harness_providers::Provider>);
    let mut input = Cursor::new(Vec::<u8>::new());
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let result = run(
        [
            "harness",
            "--session-dir",
            sessions.to_str().ok_or("session path")?,
            "prompt",
            "--mock",
            "--text",
            "Answer once",
        ],
        &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
        deps.clone(),
    );
    assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
    assert_eq!(provider.call_count(), 1);
    assert!(String::from_utf8(stdout)?.contains("A durable answer."));
    let run_dir = fs::read_dir(&sessions)?
        .next()
        .ok_or("session missing")??
        .path();
    let history = read_events(&run_dir.join("events.jsonl"))?;
    assert!(matches!(
        history.last().map(|event| &event.payload),
        Some(EventV1::RunFinished(_))
    ));
    assert!(history
        .iter()
        .any(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_))));
    assert!(!history.iter().any(|event| matches!(
        event.payload,
        EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
    )));
    let before = fs::read(run_dir.join("events.jsonl"))?;
    let invalid = root.path().join("invalid.json");
    fs::write(&invalid, "{broken")?;
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let result = run(
        [
            "harness",
            "--config",
            invalid.to_str().ok_or("config path")?,
            "replay",
            "--session",
            run_dir.to_str().ok_or("run path")?,
            "--json",
        ],
        &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
        deps,
    );
    assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
    let replay: serde_json::Value = serde_json::from_slice(&stdout)?;
    assert_eq!(replay["status"], "finished");
    assert_eq!(replay["total_events"], history.len());
    assert_eq!(provider.call_count(), 1);
    assert_eq!(fs::read(run_dir.join("events.jsonl"))?, before);
    Ok(())
}

fn serve_provider(listener: TcpListener, auto: bool) -> Result<Vec<serde_json::Value>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let listener = runtime
        .block_on(async { tokio::net::TcpListener::from_std(listener) })
        .map_err(|e| e.to_string())?;
    let mut requests = Vec::new();
    for attempt in 0..if auto { 4 } else { 2 } {
        let turn = if auto { attempt / 2 } else { attempt };
        let probe = auto && attempt % 2 == 0;
        let mut socket = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?
                .0
                .into_std()
                .map_err(|e| e.to_string())
        })?;
        socket.set_nonblocking(false).map_err(|e| e.to_string())?;
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::new(&mut socket);
        let mut headers = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).map_err(|e| e.to_string())?;
            if line == "\r\n" {
                break;
            }
            if line.is_empty() || headers.len() > 65_536 {
                return Err("invalid HTTP request".into());
            }
            headers.push_str(&line);
        }
        let route = format!(
            "POST /v1/{}{} ",
            if probe {
                "responses"
            } else {
                "chat/completions"
            },
            if auto {
                "?trace=opaque%2Dquery%2Dtoken"
            } else {
                ""
            }
        );
        if !headers.starts_with(&route)
            || !headers
                .to_ascii_lowercase()
                .contains("authorization: bearer private-fixture-token")
        {
            return Err("wrong provider route or credential".into());
        }
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|value| value.trim().parse::<usize>().ok())
            })
            .ok_or("missing request length")?;
        if length > 1024 * 1024 {
            return Err("request too large".into());
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        let request: serde_json::Value =
            serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        if probe {
            if !request["input"].is_array() {
                return Err("expected Responses request".into());
            }
            socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .map_err(|e| e.to_string())?;
            continue;
        }
        requests.push(request);
        let delta = if turn == 0 {
            json!({"tool_calls":[{"index":0,"id":"call-write","type":"function","function":{"name":"write","arguments":json!({"path":"answer.txt","content":"created"}).to_string()}}]})
        } else {
            json!({"content":format!("Finished after the tool result. private-fixture-token{}", if auto {" opaque-query-token"} else {""})})
        };
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"index":0,"delta":delta}]}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":if turn == 0 {"tool_calls"} else {"stop"}}]})
        );
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())?;
    }
    Ok(requests)
}
