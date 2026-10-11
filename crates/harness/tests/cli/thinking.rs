use super::*;
use serde_json::Value;
use std::{
    io::{BufRead, Read},
    net::TcpListener,
    time::Duration,
};

#[test]
fn thinking_tool_round_trips_keep_protocol_state_out_of_history(
) -> Result<(), Box<dyn std::error::Error>> {
    for (model, anthropic) in [
        ("claude-sonnet-4-6", true),
        ("claude-sonnet-4-5", true),
        ("gemini-3.1-pro", false),
    ] {
        let root = tempfile::tempdir()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let mut config = json!({
            "provider":{"local":{"type":if anthropic {"anthropic_messages"} else {"openai_compatible"},
                "baseURL":format!("http://{}/v1", listener.local_addr()?), "apiKeyEnv":[],
                "models":{(model):{"limit":{"context":128000,"output":16000},
                    "variants":{"high":{"metadata":{"reasoning_effort":"high"}}}}}}},
            "model":format!("local/{model}"),
            "agent":{"default":{"tools":["write"],"system_prompt":"Write the fixture."}},
            "permission":{"edit":"allow"}, "runtime":{"provider_retry":{"max_retries":0}}
        });
        if !anthropic {
            config["provider"]["local"]["apiMode"] = "chat_completions".into();
        }
        fs::write(root.path().join("runtime.json"), config.to_string())?;
        let server = std::thread::spawn(move || serve(listener, anthropic));
        let (mut input, mut stdout, mut stderr) = (Cursor::new(Vec::new()), Vec::new(), Vec::new());
        let result = run(
            [
                "harness",
                "--config",
                "runtime.json",
                "prompt",
                "--text",
                "Write answer.txt.",
                "--variant",
                "high",
            ],
            &mut CliIo::new(&mut input, &mut stdout, &mut stderr),
            CliDeps::real()
                .with_current_dir(root.path().into())
                .with_env("HARNESS_HOME", root.path().join("data").to_string_lossy())
                .without_env("HARNESS_CONFIG")
                .without_env("HARNESS_CONFIG_CONTENT"),
        );
        let requests = server.join().map_err(|_| "fixture stopped")?;
        assert_eq!(result.code, 0, "{}", String::from_utf8_lossy(&stderr));
        let requests = requests?;
        assert_eq!(
            fs::read_to_string(root.path().join("answer.txt"))?,
            "created"
        );
        let assistant = requests[1]["messages"]
            .as_array()
            .ok_or("messages missing")?
            .iter()
            .find(|message| message["role"] == "assistant")
            .ok_or("assistant missing")?;
        if anthropic {
            assert_eq!(
                assistant["content"],
                json!([
                    {"type":"thinking","thinking":"transient-thought","signature":"signed-fixture"},
                    {"type":"redacted_thinking","data":"opaque-fixture"},
                    {"type":"text","text":"Writing."},
                    {"type":"tool_use","id":"call-write","name":"write","input":{"path":"answer.txt","content":"created"}}
                ])
            );
            if model.ends_with("4-6") {
                assert_eq!(requests[0]["thinking"]["type"], "adaptive");
                assert_eq!(requests[0]["output_config"]["effort"], "high");
            } else {
                assert_eq!(requests[0]["thinking"]["type"], "enabled");
                let budget = requests[0]["thinking"]["budget_tokens"]
                    .as_u64()
                    .ok_or("budget absent")?;
                assert!((1024..16000).contains(&budget));
            }
        } else {
            assert_eq!(
                assistant["tool_calls"][0]["extra_content"]["google"]["thought_signature"],
                "signed-fixture"
            );
        }
        let sessions =
            harness_core::storage_paths::ProjectPaths::new(&root.path().join("data"), root.path())?
                .sessions_dir();
        for session in fs::read_dir(sessions)? {
            let path = session?.path();
            let mut files = vec![path.join("events.jsonl")];
            if path.join("artifacts").is_dir() {
                files.extend(
                    fs::read_dir(path.join("artifacts"))?
                        .map(|entry| entry.map(|e| e.path()))
                        .collect::<Result<Vec<_>, _>>()?,
                );
            }
            for file in files {
                let saved = fs::read_to_string(file)?;
                for marker in ["transient-thought", "signed-fixture", "opaque-fixture"] {
                    assert!(
                        !saved.contains(marker),
                        "transient provider state was persisted"
                    );
                }
            }
        }
    }
    Ok(())
}

fn serve(listener: TcpListener, anthropic: bool) -> Result<Vec<Value>, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let listener = runtime
        .block_on(async { tokio::net::TcpListener::from_std(listener) })
        .map_err(|e| e.to_string())?;
    let mut requests = Vec::new();
    for turn in 0..2 {
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
            if line.is_empty() || headers.len() > 65536 {
                return Err("invalid headers".into());
            }
            headers.push_str(&line.to_ascii_lowercase());
        }
        let size = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length:")?
                    .trim()
                    .parse::<usize>()
                    .ok()
            })
            .filter(|n| *n <= 1024 * 1024)
            .ok_or("invalid request size")?;
        let mut body = vec![0; size];
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        requests.push(serde_json::from_slice(&body).map_err(|e| e.to_string())?);
        let reply = response(anthropic, turn == 0);
        write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len()).map_err(|e| e.to_string())?;
    }
    Ok(requests)
}

fn response(anthropic: bool, tool: bool) -> String {
    let args = json!({"path":"answer.txt","content":"created"});
    let frames = if anthropic {
        let mut frames = vec![
            json!({"type":"message_start","message":{"id":"fixture","usage":{"input_tokens":10,"output_tokens":0}}}),
        ];
        if tool {
            frames.extend([
                json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"transient-thought"}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed-"}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"fixture"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"opaque-fixture"}}),
                json!({"type":"content_block_stop","index":1}),
                json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":2,"delta":{"type":"text_delta","text":"Writing."}}),
                json!({"type":"content_block_stop","index":2}),
                json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"call-write","name":"write","input":{}}}),
                json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":args.to_string()}}),
                json!({"type":"content_block_stop","index":3}),
            ]);
        } else {
            frames.extend([
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Done."}}),
                json!({"type":"content_block_stop","index":0}),
            ]);
        }
        frames.extend([
            json!({"type":"message_delta","delta":{"stop_reason":if tool {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":5}}),
            json!({"type":"message_stop"})
        ]);
        frames
    } else {
        vec![json!({"choices":[{"delta":if tool {
            json!({"tool_calls":[{"id":"call-write","function":{"name":"write","arguments":args.to_string()},"extra_content":{"google":{"thought_signature":"signed-fixture"}}}]})
        } else { json!({"content":"Done."}) },"finish_reason":if tool {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":10,"completion_tokens":5}})]
    };
    let mut body = String::new();
    for frame in frames {
        body.push_str("data: ");
        body.push_str(&frame.to_string());
        body.push_str("\n\n");
    }
    if !anthropic {
        body.push_str("data: [DONE]\n\n");
    }
    body
}
