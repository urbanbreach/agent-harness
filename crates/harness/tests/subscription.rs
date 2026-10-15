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
    for (profile, model, mode, reasoning_effort, truncated) in [
        ("openai-codex", "gpt-6-astra", "auto", "low", false),
        ("openai-codex", "gpt-6-astra", "auto", "low", true),
        ("openai-codex", "gpt-6.1-sol", "auto", "medium", false),
        (
            "github-copilot",
            "gpt-6-astra",
            "chat_completions",
            "",
            false,
        ),
        (
            "github-copilot",
            "gpt-6-astra",
            "chat_completions",
            "",
            true,
        ),
        ("copilot-claude", "claude-sonnet-4.6", "auto", "", false),
        ("copilot-claude", "claude-sonnet-4.6", "auto", "", true),
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
        CredentialStore::new(data.clone()).save(&credential)?;
        std::fs::write(root.path().join("fixture.json"), json!({
            "provider":{"fixture":{"type":"openai_compatible", "authProvider": credential.provider.as_str(),
                "baseURL":format!("http://{}/v1", listener.local_addr()?), "apiMode": mode, "apiKeyEnv":[],
                "headers":{"Authorization":"discard-this-token"},
                "models":{(model):{"limit":{"context":128000,"output":32000}}}}},
            "model":format!("fixture/{model}"), "agent":{"default":{"tools":[],"system_prompt":"Follow the fixture."}},
            "runtime":{"provider_retry":{"max_retries":0},"prompt":{"wait_timeout_ms":5000}}
        }).to_string())?;
        let server = std::thread::spawn(move || serve(listener, profile, truncated));
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
                .with_env("HARNESS_HOME", data.to_str().ok_or("data path")?)
                .with_env("HOME", root.path().to_string_lossy())
                .without_env("HARNESS_CONFIG")
                .without_env("HARNESS_CONFIG_CONTENT"),
        );
        let (headers, body) = server.join().map_err(|_| "HTTP fixture stopped")??;
        // A truncated answer that delivered text is kept with an output-limit warning.
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
        let session = std::fs::read_dir(
            harness_core::storage_paths::ProjectPaths::new(&data, root.path())?.sessions_dir(),
        )?
        .next()
        .ok_or("session missing")??
        .path();
        assert!(!std::fs::read_to_string(session.join("events.jsonl"))?
            .contains("private-subscription-token"));
        if truncated {
            assert!(String::from_utf8_lossy(&stderr).contains("output limit"));
            let events = harness_core::store::read_events(&session.join("events.jsonl"))?;
            assert!(events.iter().any(|event| matches!(&event.payload,
                harness_core::event::EventV1::ProviderRequestFinished(finished)
                if finished.usage.as_ref().is_some_and(|usage| usage.total_tokens == 10)
                    && finished.metadata.as_ref().and_then(|m| m.provider_error_category)
                        .is_some_and(|category| category.as_str() == "output_limit_exceeded")
            )));
            assert!(!events.iter().any(|event| matches!(
                &event.payload,
                harness_core::event::EventV1::ToolCallRequested(_)
            )));
        }
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

fn serve(listener: TcpListener, profile: &str, truncated: bool) -> Result<(String, Value), String> {
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
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"fixture\",\"usage\":{\"input_tokens\":7}}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Done. private-subscription-token\"}}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\ndata: {\"type\":\"message_stop\"}\n\n"
    } else {
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Done. private-subscription-token\"}}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\ndata: [DONE]\n\n"
    };
    let reply = if truncated {
        let (fragments, terminal) = if profile == "openai-codex" {
            (
                "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"partial\",\"name\":\"write\",\"arguments\":\"{\"}}\n\n",
                "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Done. private-subscription-token\"}\n\ndata: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}}\n\n".to_owned(),
            )
        } else {
            let fragment = if profile == "copilot-claude" {
                "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"partial\",\"name\":\"write\",\"input\":{}}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\"}}\n\n"
            } else {
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"partial\",\"function\":{\"name\":\"write\",\"arguments\":\"{\"}}]}}]}\n\n"
            };
            (
                fragment,
                reply
                    .replace("\"end_turn\"", "\"max_tokens\"")
                    .replace("\"stop\"", "\"length\""),
            )
        };
        format!("{fragments}{terminal}")
    } else {
        reply.to_owned()
    };
    write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).map_err(|e| e.to_string())?;
    Ok((headers, body))
}
