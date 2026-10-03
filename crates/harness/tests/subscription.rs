use harness::{run, CliDeps, CliIo};
use harness_core::auth::{CredentialStore, ProviderId, StoredCredential};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Cursor, Read, Write},
    net::TcpListener,
    time::Duration,
};

#[test]
fn subscription_prompts_use_their_wire_contract_and_redact_credentials(
) -> Result<(), Box<dyn std::error::Error>> {
    for (profile, model, mode, reasoning_effort) in [
        ("openai-codex", "gpt-6-astra", "auto", "low"),
        ("openai-codex", "gpt-6.1-sol", "auto", "medium"),
        ("github-copilot", "gpt-6-astra", "chat_completions", ""),
        ("copilot-claude", "claude-sonnet-4.6", "auto", ""),
    ] {
        let root = tempfile::tempdir()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let data = root.path().join("data");
        let mut credential = StoredCredential::oauth(
            if profile == "openai-codex" {
                ProviderId::codex()
            } else {
                ProviderId::github_copilot()
            },
            "private-subscription-token",
            "private-refresh-token",
            Some("2099-01-01T00:00:00Z".into()),
            "2026-09-26T00:00:00Z",
        );
        credential.account_id = Some("fixture-account".into());
        CredentialStore::new(data.join("harness")).save(&credential)?;
        std::fs::write(root.path().join("fixture.json"), json!({
            "provider":{"fixture":{"type":"openai_compatible", "authProvider": credential.provider.as_str(),
                "baseURL":format!("http://{}/v1", listener.local_addr()?), "apiMode": mode, "apiKeyEnv":[],
                "headers":{"Authorization":"discard-this-token"},
                "models":{(model):{"limit":{"context":128000,"output":32000}}}}},
            "model":format!("fixture/{model}"), "agent":{"default":{"tools":[],"system_prompt":"Follow the fixture."}},
            "runtime":{"provider_retry":{"max_retries":0},"prompt":{"wait_timeout_ms":5000}}
        }).to_string())?;
        let server = std::thread::spawn(move || serve(listener, profile));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                "fixture.json",
                "prompt",
                "--text",
                "Reply once.",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_env("HARNESS_DATA_HOME", data.to_str().ok_or("data path")?)
                .with_env(
                    "XDG_CONFIG_HOME",
                    root.path().to_str().ok_or("config path")?,
                )
                .without_env("HARNESS_CONFIG")
                .without_env("HARNESS_CONFIG_CONTENT"),
        );
        let (headers, body) = server.join().map_err(|_| "HTTP fixture stopped")??;
        assert_eq!(
            result.code,
            0,
            "{profile}: {}",
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(String::from_utf8(stdout)?.trim(), "Done. [REDACTED]");
        if profile == "openai-codex" {
            assert_eq!(body["reasoning"]["effort"], reasoning_effort);
        }
        check_wire(profile, &headers, &body);
        let session = std::fs::read_dir(root.path().join(".agent-harness/sessions"))?
            .next()
            .ok_or("session missing")??
            .path();
        assert!(!std::fs::read_to_string(session.join("events.jsonl"))?
            .contains("private-subscription-token"));
    }
    Ok(())
}

fn check_wire(profile: &str, headers: &str, body: &Value) {
    assert_eq!(headers.matches("authorization:").count(), 1);
    assert!(headers.contains("authorization: bearer private-subscription-token"));
    assert!(!headers.contains("discard-this-token"));
    if profile == "openai-codex" {
        check_codex(headers, body);
        return;
    }
    assert!(headers.contains("x-initiator: user"));
    assert!(headers.contains("openai-intent: conversation-edits"));
    assert!(!headers.contains("x-api-key:"));
    if profile == "copilot-claude" {
        assert!(headers.starts_with("post /v1/messages "));
        assert_eq!(body["system"], "Follow the fixture.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["max_tokens"], 32000);
    } else {
        assert!(headers.starts_with("post /v1/chat/completions "));
    }
}

fn check_codex(headers: &str, body: &Value) {
    assert!(
        headers.starts_with("post /v1/responses "),
        "wrong Codex route"
    );
    assert!(headers.contains("chatgpt-account-id: fixture-account"));
    assert!(headers.contains("originator: harness"));
    assert!(headers.contains("session-id:"));
    assert_eq!(body["instructions"], "Follow the fixture.");
    assert_eq!(body["store"], false);
    assert_eq!(body["reasoning"]["summary"], "auto");
    assert_eq!(body["text"]["verbosity"], "low");
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert!(body.get("max_output_tokens").is_none());
    assert_eq!(body["input"][0]["role"], "user");
}

fn serve(listener: TcpListener, profile: &str) -> Result<(String, Value), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let listener = runtime
        .block_on(async { tokio::net::TcpListener::from_std(listener) })
        .map_err(|e| e.to_string())?;
    let mut socket = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(6), listener.accept())
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
            return Err("invalid request".into());
        }
        headers.push_str(&line.to_ascii_lowercase());
    }
    let length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length:")?
                .trim()
                .parse::<usize>()
                .ok()
        })
        .filter(|n| *n <= 1024 * 1024)
        .ok_or("invalid request length")?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    let body = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let reply = if profile == "openai-codex" {
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Done. private-subscription-token\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"fixture\",\"status\":\"completed\"}}\n\n"
    } else if profile == "copilot-claude" {
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Done. private-subscription-token\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n"
    } else {
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Done. private-subscription-token\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
    };
    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).map_err(|e| e.to_string())?;
    Ok((headers, body))
}
