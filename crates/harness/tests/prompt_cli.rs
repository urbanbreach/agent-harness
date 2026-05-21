use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use harness_core::edit::hashline::compute_line_hash;
use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent,
    TaskTerminalScope, ToolCallFinishedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use tempfile::tempdir;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn prompt_cli_config<const N: usize>(
    base_url: &str,
    session_dir: &std::path::Path,
    tools: &[&str; N],
) -> String {
    let tools = tools.as_slice();
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": base_url,
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini",
                        "metadata": {
                            "supports_reasoning_summaries": true
                        },
                        "variants": {
                            "low": {
                                "display_name": "Low",
                                "metadata": {
                                    "reasoning_effort": "low",
                                    "text_verbosity": "low"
                                }
                            }
                        }
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "system_prompt": "You are the deep profile.",
                "model_ref": "default:gpt-4o-mini",
                "tool_failure_mode": "continue_as_tool_message",
                "tools": tools
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": session_dir,
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
    .to_string()
}

fn prompt_cli_multi_provider_config(
    default_base_url: &str,
    ops_base_url: &str,
    session_dir: &std::path::Path,
) -> String {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": default_base_url,
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini"
                    }
                }
            },
            "anthropic": {
                "type": "openai_compatible",
                "base_url": ops_base_url,
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "claude-3.7": {
                        "display_name": "Claude 3.7"
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "system_prompt": "You are the deep profile.",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            },
            "ops": {
                "description": "Ops profile",
                "system_prompt": "You are the ops profile.",
                "model_ref": "anthropic:claude-3.7",
                "tools": []
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": session_dir,
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
    .to_string()
}

fn prompt_cli_public_runtime_config(base_url: &str) -> String {
    serde_json::json!({
        "provider": {
            "default": {
                "type": "openai_compatible",
                "name": "CLIProxyAPI (OpenAI)",
                "options": {
                    "baseURL": base_url,
                    "apiKey": "DUMMY",
                    "apiMode": "responses",
                    "timeoutMs": 1800000,
                },
                "models": {
                    "gpt-5.4": {
                        "name": "GPT 5.4 (272k)",
                        "metadata": {
                            "family": "gpt-5",
                            "context_window_tokens": 272000,
                            "supports_tool_calls": true,
                            "supports_reasoning_summaries": true
                        },
                        "max_input_tokens": 272000,
                        "max_output_tokens": 128000
                    },
                    "gpt-5.4-mini": {
                        "name": "GPT 5.4 Mini",
                        "metadata": {
                            "family": "gpt-5",
                            "context_window_tokens": 272000,
                            "supports_tool_calls": true,
                            "supports_reasoning_summaries": true
                        },
                        "max_input_tokens": 272000,
                        "max_output_tokens": 128000,
                        "variants": {
                            "high": {
                                "name": "High",
                                "metadata": {
                                    "reasoning_effort": "high",
                                    "text_verbosity": "low"
                                }
                            }
                        }
                    }
                }
            }
        },
        "model": "default/gpt-5.4",
        "small_model": "default/gpt-5.4-mini",
        "agent": {
            "build": {
                "system_prompt": "You are the build profile.",
                "model": "default/gpt-5.4-mini",
                "variant": "high"
            }
        },
        "default_agent": "build",
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "question": "allow",
            "task": "allow",
            "webfetch": "allow",
            "websearch": "allow",
            "codesearch": "allow",
            "lsp": "allow"
        }
    })
    .to_string()
}

#[tokio::test]
async fn prompt_cli_calls_responses_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.test.jsonc");
    let session_dir = temp.path().join("sessions");

    let config = prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]);

    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Hello",
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert!(
        requests.iter().any(|req| req.url.path() == "/v1/responses"),
        "expected prompt CLI to call /v1/responses"
    );
}

#[tokio::test]
async fn prompt_cli_expands_at_file_and_directory_tags_for_provider() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.test.jsonc");
    let session_dir = temp.path().join("sessions");
    fs::write(temp.path().join("alpha.txt"), "alpha one\nalpha two\n").expect("write file");
    fs::create_dir(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("src/lib.rs"), "pub fn demo() {}\n").expect("write nested file");

    let config = prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]);
    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Summarize @alpha.txt and list @src",
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    let body = requests
        .iter()
        .find(|req| req.url.path() == "/v1/responses")
        .map(|req| String::from_utf8_lossy(&req.body).into_owned())
        .expect("responses request");

    assert!(body.contains("Summarize @alpha.txt and list @src"));
    assert!(body.contains("Called the Read tool with the following input"));
    assert!(body.contains("alpha.txt"));
    assert!(body.contains("1: alpha one"));
    assert!(body.contains("2: alpha two"));
    assert!(body.contains("lib.rs"));
}

#[tokio::test]
async fn prompt_cli_generates_harness_session_title() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    title_responses_sse_transcript("Debugging production 500 errors"),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.public.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");
    fs::write(
        &config_path,
        prompt_cli_public_runtime_config(&format!("{}/v1", server.uri())),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "prompt",
            "--text",
            "debug 500 errors in production",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ])
        .output()
        .expect("run harness prompt");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&out_path).expect("read prompt events");
    let events = events_body
        .lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse prompt event"))
        .collect::<Vec<_>>();
    let run_started = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::RunStarted(payload) => Some(payload),
            _ => None,
        })
        .expect("run started");
    assert!(
        harness_core::session_title::is_default_title(&run_started.run_name),
        "initial title should be harness default, got `{}`",
        run_started.run_name
    );
    assert_eq!(
        events.iter().find_map(|event| match &event.payload {
            EventV1::SessionTitleUpdated(payload) => Some(payload.title.as_str()),
            _ => None,
        }),
        Some("Debugging production 500 errors")
    );

    let meta_path = session_dir.join(&events[0].run_id).join("meta.json");
    let meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&meta_path).expect("read meta"))
            .expect("parse meta");
    assert_eq!(meta["run_name"], "Debugging production 500 errors");
    assert_eq!(meta["mode_source"], "prompt");

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(
        requests.len(),
        2,
        "expected title request plus main prompt request"
    );
    let first_request_body = String::from_utf8_lossy(&requests[0].body);
    assert!(
        first_request_body.contains("Generate a title for this conversation:"),
        "first provider request should be the harness title request: {first_request_body}"
    );
}

#[tokio::test]
async fn prompt_tracker_waits_for_agent_turn_end_not_provider_finish() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.prompt-tracker.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");

    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "Hello",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ])
        .output()
        .expect("run harness prompt");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Hello"),
        "prompt output should still print provider text deltas after provider-call ids diverge from turn ids:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let events_body = fs::read_to_string(&out_path).expect("read prompt events");
    let events = events_body
        .lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse prompt event"))
        .collect::<Vec<_>>();

    let turn_request_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::UserMessageSubmitted(payload) => Some(payload.request_id.as_str()),
            _ => None,
        })
        .expect("turn request id");

    let provider_finished_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(payload)
                if payload.finish_reason.eq_ignore_ascii_case("done") =>
            {
                assert_eq!(
                    event.correlation_id.as_deref(),
                    Some(turn_request_id),
                    "provider finish should be correlated to the stable agent turn id"
                );
                assert_ne!(
                    payload.request_id, turn_request_id,
                    "provider finish payload id is the provider-call id, not the prompt completion id"
                );
                Some(event.seq)
            }
            _ => None,
        })
        .expect("provider finish event");
    let agent_turn_completed_seq = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload)
                if payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.task_scope)
                    .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn)) =>
            {
                Some(event.seq)
            }
            _ => None,
        })
        .expect("agent turn task completion event");

    assert!(
        provider_finished_seq < agent_turn_completed_seq,
        "provider finish alone must not be treated as prompt completion; events:\n{events_body}"
    );
}

#[tokio::test]
async fn prompt_cli_accepts_public_slash_style_model_refs() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.public.jsonc");

    fs::write(
        &config_path,
        prompt_cli_public_runtime_config(&format!("{}/v1", server.uri())),
    )
    .expect("write public config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Hello",
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert!(
        requests.iter().any(|req| req.url.path() == "/v1/responses"),
        "expected prompt CLI to call /v1/responses"
    );
}

#[tokio::test]
async fn prompt_cli_creates_durable_run_logs_under_run_dir() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.logging.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "Hello",
            "--print-run-dir",
        ])
        .output()
        .expect("run harness prompt with logging");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let run_dir_line = stdout
        .lines()
        .rev()
        .find(|line| line.contains("prompt_") || line.contains("run_"))
        .expect("run dir line in prompt output");
    let log_path = std::path::Path::new(run_dir_line)
        .join("logs")
        .join("harness.log");
    assert!(
        log_path.exists(),
        "expected log file at {}",
        log_path.display()
    );

    let log_body = fs::read_to_string(&log_path).expect("read harness log file");
    assert!(
        log_body.contains("initialized harness file logging"),
        "expected logging init marker in {}\n{}",
        log_path.display(),
        log_body
    );
}

#[tokio::test]
async fn prompt_cli_model_variant_and_thinking_flags_stream_reasoning_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "\"reasoning\":{\"effort\":\"low\",\"summary\":\"auto\"}",
        ))
        .and(body_string_contains("\"text\":{\"verbosity\":\"low\"}"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(reasoning_responses_sse_transcript(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.reasoning.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "Hello",
            "--model",
            "default:gpt-4o-mini",
            "--variant",
            "low",
            "--thinking",
        ])
        .output()
        .expect("run harness prompt with thinking");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Thinking: Drafting a careful answer."));
    assert!(stdout.contains("Hello world"));
}

#[tokio::test]
async fn prompt_cli_thinking_prints_late_reasoning_before_one_assistant_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    late_reasoning_duplicate_body_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.late-reasoning.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "hi",
            "--thinking",
        ])
        .output()
        .expect("run harness prompt with late thinking");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let thinking = "Thinking: Responding to greetings";
    let body = "Hi! How can I help?";
    let thinking_index = stdout.find(thinking).expect("thinking output");
    let body_index = stdout.find(body).expect("assistant output");
    assert!(
        thinking_index < body_index,
        "thinking should print before assistant body:\n{stdout}"
    );
    assert_eq!(stdout.matches(body).count(), 1, "{stdout}");
}

#[tokio::test]
async fn prompt_cli_thinking_preserves_repeated_body_chunks_before_reasoning() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    repeated_body_chunks_before_reasoning_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.repeated-chunks.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "repeat",
            "--thinking",
        ])
        .output()
        .expect("run harness prompt with repeated chunks");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Thinking: Done planning."), "{stdout}");
    assert!(stdout.contains("ha ha"), "{stdout}");
}

#[test]
fn models_cli_lists_configured_variants() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.models.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config("http://127.0.0.1:9999/v1", &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "models",
        ])
        .output()
        .expect("run harness models");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("default:gpt-4o-mini | label=GPT-4o mini"));
    assert!(stdout.contains("default:gpt-4o-mini | variant=low | label=GPT-4o mini · Low"));
}

#[test]
fn prompt_cli_mock_mode_runs_without_config() {
    let temp = tempdir().expect("tempdir");
    let out_path = temp.path().join("events.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "prompt",
            "--mock",
            "--text",
            "Hello from PTY",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ])
        .output()
        .expect("run harness prompt --mock");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&out_path).expect("read prompt events");
    assert!(
        events_body.contains("\"event_type\":\"task_completed\""),
        "expected prompt mock run to complete a task: {events_body}"
    );
    assert!(
        events_body.contains("Hello world"),
        "expected prompt mock transcript to include the scripted provider response: {events_body}"
    );
}

#[test]
fn prompt_cli_mock_mode_accepts_positional_text() {
    let temp = tempdir().expect("tempdir");
    let out_path = temp.path().join("events-positional.jsonl");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "prompt",
            "--mock",
            "Hello from PTY",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ])
        .output()
        .expect("run harness prompt with positional text");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(&out_path).expect("read prompt events");
    assert!(events_body.contains("Hello world"));
}

#[test]
fn prompt_cli_mock_mode_accepts_stdin_text() {
    let temp = tempdir().expect("tempdir");
    let out_path = temp.path().join("events-stdin.jsonl");

    let mut child = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "prompt",
            "--mock",
            "--stdin",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn prompt stdin command");

    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(b"Hello from PTY\n")
        .expect("write stdin prompt");

    let output = child.wait_with_output().expect("wait for stdin prompt");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(&out_path).expect("read prompt events");
    assert!(events_body.contains("Hello world"));
}

#[tokio::test]
async fn prompt_cli_uses_merged_xdg_and_local_config_without_explicit_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let xdg_root = temp.path().join("xdg");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");
    let local_config_path = temp.path().join("harness.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::create_dir_all(xdg_config_path.parent().expect("xdg config parent"))
        .expect("create xdg config dir");
    fs::write(
        &xdg_config_path,
        serde_json::json!({
            "providers": {
                "default": {
                    "type": "openai_compatible",
                    "base_url": format!("{}/v1", server.uri()),
                    "api_key": "DUMMY",
                    "api_mode": "responses",
                    "timeout_ms": 60000,
                    "models": {
                        "gpt-4o-mini": {
                            "display_name": "GPT-4o mini"
                        }
                    }
                }
            },
            "permissions": {
                "defaults": {
                    "edit": "allow",
                    "shell": "allow",
                    "network": "allow"
                }
            },
            "runtime": {
                "background_tasks": {
                    "default_concurrency": 2,
                    "provider_concurrency": 2,
                    "model_concurrency": 2,
                    "stale_timeout_ms": 30000,
                    "message_staleness_timeout_ms": 10000
                },
                "session_dir": session_dir,
                "deterministic": {
                    "enabled": false,
                    "seed": 42
                }
            },
            "integrations": {
                "remote_search": {
                    "endpoint": "https://mcp.exa.ai/mcp"
                }
            }
        })
        .to_string(),
    )
    .expect("write xdg config");
    fs::write(
        &local_config_path,
        serde_json::json!({
            "agents": {
                "deep": {
                    "description": "Deep profile",
                    "system_prompt": "You are the deep profile.",
                    "model_ref": "default:gpt-4o-mini",
                    "tools": []
                }
            },
            "ui": {
                "default_profile": "deep"
            }
        })
        .to_string(),
    )
    .expect("write local config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", &xdg_root)
        .args(["prompt", "Hello from merged config"])
        .output()
        .expect("run prompt with merged config discovery");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert!(
        requests.iter().any(|req| req.url.path() == "/v1/responses"),
        "expected merged config prompt CLI to call /v1/responses"
    );
}

#[tokio::test]
async fn prompt_cli_resume_flag_continues_existing_session() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("Continue from the saved session."))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    deterministic_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.resume.jsonc");
    let session_dir = temp.path().join("sessions");
    let resume_dir = session_dir.join("run_resume_cli");
    fs::create_dir_all(&resume_dir).expect("create resume run dir");
    fs::create_dir_all(temp.path().join("workspace")).expect("create workspace");
    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");
    write_resume_fixture_events(&resume_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--resume",
            "run_resume_cli",
            "--text",
            "Continue from the saved session.",
            "--print-run-dir",
        ])
        .output()
        .expect("run harness prompt resume");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("run_resume_cli"),
        "expected resumed run dir in stdout, got:\n{stdout}"
    );

    let events_body =
        fs::read_to_string(resume_dir.join("events.jsonl")).expect("read resumed events");
    assert!(events_body.contains("\"request_id\":\"req_000002\""));
    assert!(events_body.contains("Continue from the saved session."));

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(requests.len(), 1, "expected one resumed provider request");
}

#[tokio::test]
async fn prompt_cli_routes_non_default_profile_to_matching_provider() {
    let default_server = MockServer::start().await;
    let ops_server = MockServer::start().await;

    for server in [&default_server, &ops_server] {
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_raw(
                        deterministic_responses_sse_transcript(),
                        "text/event-stream",
                    ),
            )
            .mount(server)
            .await;
    }

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.multi-provider.jsonc");
    let session_dir = temp.path().join("sessions");

    let config = prompt_cli_multi_provider_config(
        &format!("{}/v1", default_server.uri()),
        &format!("{}/v1", ops_server.uri()),
        &session_dir,
    );
    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--profile",
                "ops",
                "--text",
                "Hello from ops",
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let default_requests = default_server
        .received_requests()
        .await
        .expect("default request recording must be enabled");
    let ops_requests = ops_server
        .received_requests()
        .await
        .expect("ops request recording must be enabled");

    assert!(
        default_requests.is_empty(),
        "non-default prompt profile should not hit providers.default"
    );
    assert_eq!(
        ops_requests
            .iter()
            .filter(|req| req.url.path() == "/v1/responses")
            .count(),
        1,
        "expected prompt CLI to hit the selected non-default provider exactly once"
    );
}

#[tokio::test]
async fn prompt_cli_executes_tool_call_and_completes_turn() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_result_followup_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "Read tool-target.txt and then summarize it.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_responses_sse_transcript(), "text/event-stream"),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.tool-loop.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");
    fs::write(temp.path().join("tool-target.txt"), "alpha\nbeta\ngamma\n")
        .expect("seed tool target");

    let config = prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &["read"]);

    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let out_arg = out_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Read tool-target.txt and then summarize it.",
                "--out",
                out_arg.to_str().expect("out path utf-8"),
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&out_path).expect("read prompt events");
    assert!(events_body.contains("\"event_type\":\"tool_call_requested\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_started\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"status\":\"succeeded\""));
    assert!(events_body.contains("tool-target.txt"));
    assert!(
        events_body.contains("alpha")
            || events_body.contains("beta")
            || events_body.contains("gamma")
    );

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(
        requests.len(),
        2,
        "expected tool loop to require two provider requests"
    );

    let second_body: serde_json::Value = requests[1]
        .body_json()
        .expect("second request body must be JSON");
    let input = second_body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("responses request should contain input array");

    let function_call_index = input
        .iter()
        .position(|item| {
            item.get("type") == Some(&serde_json::Value::String("function_call".to_string()))
        })
        .expect("expected follow-up request to include a function_call replay item");
    let function_call_output_index = input
        .iter()
        .position(|item| {
            item.get("type")
                == Some(&serde_json::Value::String(
                    "function_call_output".to_string(),
                ))
        })
        .expect("expected follow-up request to include a function_call_output item");

    assert!(
        function_call_index < function_call_output_index,
        "function_call replay must appear before function_call_output: {second_body}"
    );
    assert_eq!(
        input[function_call_index].get("call_id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );
    assert_eq!(
        input[function_call_output_index].get("call_id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );

    let arguments = input[function_call_index]
        .get("arguments")
        .and_then(serde_json::Value::as_str)
        .expect("function_call replay item should include serialized arguments");
    let parsed_arguments: serde_json::Value =
        serde_json::from_str(arguments).expect("function_call arguments should be JSON");
    assert_eq!(
        parsed_arguments.get("path"),
        Some(&serde_json::Value::String("tool-target.txt".to_string()))
    );
}

#[tokio::test]
async fn prompt_cli_black_box_agent_turn_oracle_records_artifact_and_replays() {
    let server = MockServer::start().await;
    let prompt_text = "Run the deterministic artifact oracle and report the final marker.";

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .and(body_string_contains("call_oracle_bash"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript("ORACLE_DONE artifact captured."),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(prompt_text))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    bash_artifact_call_responses_sse_transcript(),
                    "text/event-stream",
                ),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.agent-turn-oracle.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");

    let config = prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &["bash"]);
    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let out_arg = out_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                prompt_text,
                "--out",
                out_arg.to_str().expect("out path utf-8"),
                "--print-run-dir",
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ORACLE_DONE artifact captured."),
        "prompt should stream final assistant text to stdout: {stdout}"
    );
    let run_dir_line = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('/'))
        .expect("--print-run-dir should print an absolute run directory");
    let run_dir = std::path::PathBuf::from(run_dir_line);
    assert!(run_dir.join("events.jsonl").is_file());

    let events_body = fs::read_to_string(&out_path).expect("read copied prompt events");
    let persisted_events_body =
        fs::read_to_string(run_dir.join("events.jsonl")).expect("read persisted prompt events");
    assert_eq!(
        events_body, persisted_events_body,
        "--out must be an exact copy of the persisted event log"
    );

    let events = parse_prompt_events(&events_body);
    assert!(!events.is_empty());
    for (idx, event) in events.iter().enumerate() {
        assert_eq!(event.schema_version, SCHEMA_VERSION);
        assert_eq!(
            event.seq,
            idx as u64 + 1,
            "event sequence must be contiguous"
        );
        assert_eq!(event.run_id, events[0].run_id);
    }
    assert_eq!(run_dir, session_dir.join(&events[0].run_id));

    assert_eq!(
        events.iter().find_map(|event| match &event.payload {
            EventV1::UserMessageSubmitted(payload) => Some(payload.text.as_str()),
            _ => None,
        }),
        Some(prompt_text)
    );
    assert_eq!(
        events.iter().find_map(|event| match &event.payload {
            EventV1::AgentSpawned(payload) => Some(payload.profile.as_str()),
            _ => None,
        }),
        Some("deep")
    );

    let provider_started_positions = event_positions(&events, |event| {
        matches!(event.payload, EventV1::ProviderRequestStarted(_))
    });
    let provider_finished_positions = event_positions(&events, |event| {
        matches!(event.payload, EventV1::ProviderRequestFinished(_))
    });
    assert_eq!(provider_started_positions.len(), 2);
    assert_eq!(provider_finished_positions.len(), 2);

    let tool_ready_pos = first_event_position(
        &events,
        |event| matches!(&event.payload, EventV1::AssistantMessageFinished(payload) if payload.tool_call_count == 1),
        "assistant message with tool call",
    );
    let tool_requested_pos = first_event_position(
        &events,
        |event| matches!(&event.payload, EventV1::ToolCallRequested(payload) if payload.tool_id == "bash"),
        "bash tool request",
    );
    let requested_tool = match &events[tool_requested_pos].payload {
        EventV1::ToolCallRequested(payload) => payload,
        _ => unreachable!("tool_requested_pos must point at tool_call_requested"),
    };
    assert_eq!(requested_tool.tool_id, "bash");
    assert!(requested_tool.args_summary.contains("printf"));
    let tool_call_id = requested_tool.tool_call_id.clone();

    let tool_started_pos = first_event_position(
        &events,
        |event| matches!(&event.payload, EventV1::ToolCallStarted(payload) if payload.tool_call_id == tool_call_id),
        "bash tool start",
    );
    let tool_finished_pos = first_event_position(
        &events,
        |event| matches!(&event.payload, EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id),
        "bash tool finish",
    );
    let tool_task_completed_pos = first_event_position(
        &events,
        |event| {
            matches!(&event.payload, EventV1::TaskCompleted(payload)
                if payload.metadata.as_ref().and_then(|metadata| metadata.task_scope)
                    .is_some_and(|scope| matches!(scope, TaskTerminalScope::ToolCall)))
        },
        "tool task completion",
    );
    let agent_turn_completed_pos = first_event_position(
        &events,
        |event| {
            matches!(&event.payload, EventV1::TaskCompleted(payload)
                if payload.metadata.as_ref().and_then(|metadata| metadata.task_scope)
                    .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn)))
        },
        "agent turn completion",
    );
    let run_finished_pos = first_event_position(
        &events,
        |event| matches!(event.payload, EventV1::RunFinished(_)),
        "run finished",
    );

    assert!(provider_started_positions[0] < provider_finished_positions[0]);
    assert!(provider_finished_positions[0] < tool_ready_pos);
    assert!(tool_ready_pos < tool_requested_pos);
    assert!(tool_requested_pos < tool_started_pos);
    assert!(tool_started_pos < tool_finished_pos);
    assert!(tool_started_pos < tool_task_completed_pos);
    assert!(tool_finished_pos < provider_started_positions[1]);
    assert!(tool_task_completed_pos < provider_started_positions[1]);
    assert!(provider_started_positions[1] < provider_finished_positions[1]);
    assert!(provider_finished_positions[1] < agent_turn_completed_pos);
    assert!(agent_turn_completed_pos < run_finished_pos);

    let finished_tool = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload)
            }
            _ => None,
        })
        .expect("bash tool finished event");
    assert_eq!(finished_tool.status, ToolCallStatus::Succeeded);
    let output_json = finished_tool
        .output_json
        .as_ref()
        .expect("bash tool should persist structured output metadata");
    assert_eq!(output_json.get("truncated"), Some(&serde_json::json!(true)));
    assert_eq!(
        output_json.get("total_output_bytes"),
        Some(&serde_json::json!(66_000))
    );
    let output_artifact = output_json
        .get("output_artifact")
        .and_then(serde_json::Value::as_object)
        .expect("truncated bash output should reference an artifact");
    let artifact_path = output_artifact
        .get("path")
        .and_then(serde_json::Value::as_str)
        .expect("output artifact path");
    let artifact_digest = output_artifact
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .expect("output artifact digest");
    assert!(artifact_path.starts_with("artifacts/toolcalls/"));
    assert!(artifact_path.contains(&tool_call_id));

    let artifact_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ArtifactWritten(payload) if payload.path == artifact_path => Some(payload),
            _ => None,
        })
        .expect("artifact_written event for spilled bash output");
    assert_eq!(
        artifact_event.tool_call_id.as_deref(),
        Some(tool_call_id.as_str())
    );
    assert_eq!(artifact_event.digest, artifact_digest);
    assert_eq!(artifact_event.bytes, 66_000);

    let artifact_body = fs::read_to_string(run_dir.join(artifact_path)).expect("read artifact");
    assert_eq!(artifact_body.len() as u64, artifact_event.bytes);
    assert!(artifact_body.starts_with("oracleoracleoracle"));

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(
        requests.len(),
        2,
        "oracle should require exactly two provider turns"
    );
    let first_body = String::from_utf8_lossy(&requests[0].body);
    assert!(first_body.contains(prompt_text));
    assert!(first_body.contains("\"name\":\"bash\""));
    assert!(!first_body.contains("function_call_output"));

    let second_body = String::from_utf8_lossy(&requests[1].body);
    assert!(second_body.contains("\"type\":\"function_call\""));
    assert!(second_body.contains("\"type\":\"function_call_output\""));
    assert!(second_body.contains("call_oracle_bash"));
    assert!(second_body.contains(artifact_path));
    assert!(second_body.contains("[truncated:"));

    let replay_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json");
    assert!(
        replay_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&replay_output.stdout),
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).expect("replay json output should parse");
    assert_eq!(replay["run_id"], serde_json::json!(events[0].run_id));
    assert_eq!(replay["status"], serde_json::json!("finished"));
    assert_eq!(replay["artifact_count"], serde_json::json!(1));
    assert_eq!(
        replay["counts_by_type"]["artifact_written"],
        serde_json::json!(1)
    );
    let replay_artifacts = replay["artifacts"]
        .as_array()
        .expect("replay artifacts array");
    assert!(replay_artifacts.iter().any(|artifact| {
        artifact.get("path") == Some(&serde_json::json!(artifact_path))
            && artifact.get("tool_call_id") == Some(&serde_json::json!(tool_call_id))
            && artifact.get("present_on_disk") == Some(&serde_json::json!(true))
    }));
}

#[tokio::test]
async fn prompt_cli_black_box_workspace_mutation_oracle_recovers_and_replays() {
    let server = MockServer::start().await;
    let prompt_text = "Read src/status.txt, change status to done, verify, try the bad shell read, recover, and report.";

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("call_workspace_bad_bash"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript(
                        "WORKSPACE_ORACLE_DONE status updated and failed shell read recovered.",
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("call_workspace_bash"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_call_sse_transcript(
                        "item_workspace_bad_bash",
                        "call_workspace_bad_bash",
                        "bash",
                        serde_json::json!({
                            "command": "cat src/status.txt",
                            "workdir": ".",
                            "description": "bad shell read should be blocked",
                            "timeout": 120000,
                        }),
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("call_workspace_edit"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_call_sse_transcript(
                        "item_workspace_bash",
                        "call_workspace_bash",
                        "bash",
                        serde_json::json!({
                            "command": "test -f src/status.txt && printf verified",
                            "workdir": ".",
                            "description": "verify edited status file exists",
                            "timeout": 120000,
                        }),
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(3)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("call_workspace_read"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_call_sse_transcript(
                        "item_workspace_edit",
                        "call_workspace_edit",
                        "edit",
                        serde_json::json!({
                            "filePath": "src/status.txt",
                            "editId": "workspace-status-done",
                            "edits": [
                                {
                                    "op": "replace",
                                    "pos": format!("2#{}", compute_line_hash("status = \"pending\"")),
                                    "lines": ["status = \"done\""],
                                }
                            ],
                        }),
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(4)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(prompt_text))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_call_sse_transcript(
                        "item_workspace_read",
                        "call_workspace_read",
                        "read",
                        serde_json::json!({
                            "path": "src/status.txt",
                            "offset": 1,
                            "limit": 10,
                        }),
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(5)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("src")).expect("create src dir");
    let status_path = temp.path().join("src/status.txt");
    fs::write(
        &status_path,
        "title = \"harness\"\nstatus = \"pending\"\nchecks = [\"read\"]\n",
    )
    .expect("seed status file");

    let config_path = temp.path().join("harness.workspace-oracle.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");
    let config = prompt_cli_config(
        &format!("{}/v1", server.uri()),
        &session_dir,
        &["read", "edit", "bash"],
    );
    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let out_arg = out_path.clone();
    let temp_path = temp.path().to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                prompt_text,
                "--out",
                out_arg.to_str().expect("out path utf-8"),
                "--print-run-dir",
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("WORKSPACE_ORACLE_DONE status updated and failed shell read recovered."),
        "prompt should stream final recovery text: {stdout}"
    );

    assert_eq!(
        fs::read_to_string(&status_path).expect("read mutated status file"),
        "title = \"harness\"\nstatus = \"done\"\nchecks = [\"read\"]\n"
    );

    let run_dir_line = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with('/'))
        .expect("--print-run-dir should print an absolute run directory");
    let run_dir = std::path::PathBuf::from(run_dir_line);
    assert!(run_dir.join("events.jsonl").is_file());

    let events_body = fs::read_to_string(&out_path).expect("read copied prompt events");
    assert_eq!(
        events_body,
        fs::read_to_string(run_dir.join("events.jsonl")).expect("read persisted prompt events"),
        "--out must match the persisted event log"
    );
    let events = parse_prompt_events(&events_body);
    assert!(!events.is_empty());
    for (idx, event) in events.iter().enumerate() {
        assert_eq!(event.schema_version, SCHEMA_VERSION);
        assert_eq!(event.seq, idx as u64 + 1);
        assert_eq!(event.run_id, events[0].run_id);
    }

    let read_request_pos = tool_request_position(&events, "read", "src/status.txt");
    let edit_request_pos = tool_request_position(&events, "edit", "workspace-status-done");
    let verify_bash_request_pos = tool_request_position(&events, "bash", "printf verified");
    let blocked_bash_request_pos = tool_request_position(&events, "bash", "cat src/status.txt");
    assert!(read_request_pos < edit_request_pos);
    assert!(edit_request_pos < verify_bash_request_pos);
    assert!(verify_bash_request_pos < blocked_bash_request_pos);

    let read_tool_call_id = requested_tool_call_id(&events[read_request_pos]);
    let edit_tool_call_id = requested_tool_call_id(&events[edit_request_pos]);
    let verify_bash_tool_call_id = requested_tool_call_id(&events[verify_bash_request_pos]);
    let blocked_bash_tool_call_id = requested_tool_call_id(&events[blocked_bash_request_pos]);
    assert_tool_finished(&events, &read_tool_call_id, ToolCallStatus::Succeeded);
    assert_tool_finished(&events, &edit_tool_call_id, ToolCallStatus::Succeeded);
    assert_tool_finished(
        &events,
        &verify_bash_tool_call_id,
        ToolCallStatus::Succeeded,
    );
    let blocked_bash_finish =
        assert_tool_finished(&events, &blocked_bash_tool_call_id, ToolCallStatus::Failed);
    assert!(
        blocked_bash_finish
            .output_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("text-processing shell commands are blocked")),
        "blocked bash failure should guide recovery: {blocked_bash_finish:?}"
    );

    let edit_applied = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::EditApplied(payload) if payload.edit_id == "workspace-status-done" => {
                Some(payload)
            }
            _ => None,
        })
        .expect("edit_applied event for workspace mutation");
    assert_eq!(edit_applied.path, "src/status.txt");
    let edit_diff_rel_path = edit_applied
        .diff_rel_path
        .as_deref()
        .expect("edit should write a replayable diff artifact");
    assert!(run_dir.join(edit_diff_rel_path).is_file());
    assert!(events.iter().any(|event| {
        matches!(&event.payload, EventV1::ArtifactWritten(payload)
            if payload.path == edit_diff_rel_path
                && payload.tool_call_id.as_deref() == Some(edit_tool_call_id.as_str()))
    }));
    assert!(events.iter().any(|event| {
        matches!(&event.payload, EventV1::EditProposed(payload)
            if payload.edit_id == "workspace-status-done" && payload.path == "src/status.txt")
    }));
    assert!(!events.iter().any(|event| {
        matches!(&event.payload, EventV1::EditRejected(payload)
            if payload.edit_id == "workspace-status-done")
    }));

    let final_answer_pos = events
        .iter()
        .enumerate()
        .skip(blocked_bash_request_pos)
        .find_map(|(idx, event)| match &event.payload {
            EventV1::AssistantMessageFinished(payload) if payload.tool_call_count == 0 => Some(idx),
            _ => None,
        })
        .expect("final assistant recovery answer");
    let agent_turn_completed_pos = first_event_position(
        &events,
        |event| {
            matches!(&event.payload, EventV1::TaskCompleted(payload)
                if payload.metadata.as_ref().and_then(|metadata| metadata.task_scope)
                    .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn)))
        },
        "agent turn completion",
    );
    assert!(blocked_bash_request_pos < final_answer_pos);
    assert!(final_answer_pos < agent_turn_completed_pos);

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(
        requests.len(),
        5,
        "workspace oracle should require four tool turns plus final answer"
    );
    let first_body = String::from_utf8_lossy(&requests[0].body);
    assert!(first_body.contains(prompt_text));
    assert!(first_body.contains("\"name\":\"read\""));
    assert!(first_body.contains("\"name\":\"edit\""));
    assert!(first_body.contains("\"name\":\"bash\""));
    for (idx, call_id) in [
        "call_workspace_read",
        "call_workspace_edit",
        "call_workspace_bash",
        "call_workspace_bad_bash",
    ]
    .iter()
    .enumerate()
    {
        let body = String::from_utf8_lossy(&requests[idx + 1].body);
        assert!(
            body.contains(call_id),
            "follow-up request {} should contain provider call id {call_id}: {body}",
            idx + 1
        );
        assert_request_has_function_call_output(&requests[idx + 1].body, call_id);
    }
    assert!(String::from_utf8_lossy(&requests[4].body)
        .contains("text-processing shell commands are blocked"));

    let events_before_replay =
        fs::read_to_string(run_dir.join("events.jsonl")).expect("read event log before replay");
    let replay_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json");
    assert!(
        replay_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&replay_output.stdout),
        String::from_utf8_lossy(&replay_output.stderr)
    );
    assert_eq!(
        events_before_replay,
        fs::read_to_string(run_dir.join("events.jsonl")).expect("read event log after replay"),
        "replay must not mutate the event log"
    );
    assert_eq!(
        fs::read_to_string(&status_path).expect("read status file after replay"),
        "title = \"harness\"\nstatus = \"done\"\nchecks = [\"read\"]\n",
        "replay must not rerun tools or mutate the workspace"
    );
    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).expect("replay json output should parse");
    assert_eq!(replay["run_id"], serde_json::json!(events[0].run_id));
    assert_eq!(replay["status"], serde_json::json!("finished"));
    assert_eq!(
        replay["counts_by_type"]["edit_applied"],
        serde_json::json!(1)
    );
    assert_eq!(
        replay["counts_by_type"]["tool_call_finished"],
        serde_json::json!(4)
    );
    assert_eq!(replay["tasks_in_flight"], serde_json::json!([]));
    let replay_artifacts = replay["artifacts"]
        .as_array()
        .expect("replay artifacts array");
    assert!(replay_artifacts.iter().any(|artifact| {
        artifact.get("path") == Some(&serde_json::json!(edit_diff_rel_path))
            && artifact.get("tool_call_id") == Some(&serde_json::json!(edit_tool_call_id))
            && artifact.get("present_on_disk") == Some(&serde_json::json!(true))
    }));
}

#[tokio::test]
async fn prompt_cli_exits_nonzero_on_provider_error_finish() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(provider_error_sse_transcript(), "text/event-stream"),
        )
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.provider-error.jsonc");
    let session_dir = temp.path().join("sessions");
    fs::write(
        &config_path,
        prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &[]),
    )
    .expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "Trigger a provider error.",
        ])
        .output()
        .expect("run harness prompt with provider error");

    assert!(
        !output.status.success(),
        "provider error finish must exit nonzero\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("prompt failed"),
        "stderr should report prompt failure:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let run_dirs = fs::read_dir(&session_dir)
        .expect("read session dir")
        .map(|entry| entry.expect("session dir entry").path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    assert_eq!(run_dirs.len(), 1, "expected one prompt run dir");
    let events_body =
        fs::read_to_string(run_dirs[0].join("events.jsonl")).expect("read provider error events");
    assert!(events_body.contains("\"event_type\":\"provider_request_finished\""));
    assert!(events_body.contains("\"finish_reason\":\"error\""));
    assert!(events_body.contains("\"event_type\":\"task_cancelled\""));
}

#[tokio::test]
async fn prompt_cli_continues_after_tool_failure_as_tool_message() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript("Recovered after the failed read tool call."),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "Read missing-tool-target.txt and recover.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_missing_read_sse_transcript(), "text/event-stream"),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    let output = run_prompt_with_single_tool(
        temp.path(),
        &server,
        &["read"],
        "Read missing-tool-target.txt and recover.",
    )
    .await;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("Recovered after the failed read tool call."));

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"status\":\"failed\""));
    assert!(events_body.contains("missing-tool-target.txt"));

    let requests = server
        .received_requests()
        .await
        .expect("request recording must be enabled");
    assert_eq!(
        requests.len(),
        2,
        "tool failure should be returned as a tool message and followed by a second provider request"
    );
    let second_body: serde_json::Value = requests[1]
        .body_json()
        .expect("second request body must be JSON");
    let input = second_body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("responses request should contain input array");
    let tool_output = input
        .iter()
        .find(|item| item.get("type") == Some(&serde_json::json!("function_call_output")))
        .expect("follow-up request includes failed function_call_output");
    assert_eq!(
        tool_output.get("call_id"),
        Some(&serde_json::json!("call_missing"))
    );
    assert!(
        tool_output
            .get("output")
            .and_then(serde_json::Value::as_str)
            .expect("function call output text")
            .contains("tool call `read` failed"),
        "failed tool result should be sent back to the provider: {second_body}"
    );
}

#[tokio::test]
async fn prompt_cli_executes_fs_glob_and_completes_turn() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript(
                        "Glob complete: fixtures/a.txt and fixtures/nested/b.txt.",
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "Use glob on fixtures and summarize the matches.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_glob_sse_transcript(), "text/event-stream"),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("fixtures/nested")).expect("create fixtures tree");
    fs::write(temp.path().join("fixtures/a.txt"), "alpha\n").expect("write a.txt");
    fs::write(temp.path().join("fixtures/nested/b.txt"), "beta\n").expect("write b.txt");
    fs::write(temp.path().join("fixtures/c.md"), "ignore\n").expect("write c.md");

    let output = run_prompt_with_single_tool(
        temp.path(),
        &server,
        &["glob"],
        "Use glob on fixtures and summarize the matches.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "glob");
    assert!(events_body.contains("fixtures/a.txt"));
    assert!(events_body.contains("fixtures/nested/b.txt"));
}

#[tokio::test]
async fn prompt_cli_executes_fs_ls_and_completes_turn() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript(
                        "Directory listing complete: alpha/, beta.txt, zeta.log.",
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "Use list on fixtures and summarize the entries.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_list_sse_transcript(), "text/event-stream"),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("fixtures/alpha")).expect("create alpha dir");
    fs::write(temp.path().join("fixtures/beta.txt"), "beta\n").expect("write beta.txt");
    fs::write(temp.path().join("fixtures/zeta.log"), "zeta\n").expect("write zeta.log");

    let output = run_prompt_with_single_tool(
        temp.path(),
        &server,
        &["list"],
        "Use list on fixtures and summarize the entries.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "list");
    assert!(events_body.contains("alpha/"));
    assert!(events_body.contains("beta.txt"));
    assert!(events_body.contains("zeta.log"));
}

#[tokio::test]
async fn prompt_cli_executes_fs_grep_and_completes_turn() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript(
                        "Grep complete: fixtures/notes.md contains BETA on line 2.",
                    ),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "Use grep in fixtures for BETA and summarize the hit.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_grep_sse_transcript(), "text/event-stream"),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("fixtures")).expect("create fixtures dir");
    fs::write(
        temp.path().join("fixtures/notes.md"),
        "alpha\nBETA match\ngamma\n",
    )
    .expect("write notes.md");
    fs::write(temp.path().join("fixtures/skip.txt"), "BETA hidden\n").expect("write skip.txt");

    let output = run_prompt_with_single_tool(
        temp.path(),
        &server,
        &["grep"],
        "Use grep in fixtures for BETA and summarize the hit.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "grep");
    assert!(events_body.contains("fixtures/notes.md:2: BETA match"));
}

#[tokio::test]
async fn prompt_cli_reads_absolute_workspace_path_and_completes_turn() {
    let server = MockServer::start().await;
    let temp = tempdir().expect("tempdir");
    let absolute_target = temp.path().join("tool-target.txt");
    fs::write(&absolute_target, "alpha\nbeta\ngamma\n").expect("seed tool target");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains("\"type\":\"function_call_output\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_followup_text_sse_transcript("Absolute read complete: alpha beta gamma."),
                    "text/event-stream",
                ),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(body_string_contains(
            "Read the absolute tool-target.txt path and summarize it.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(
                    tool_call_absolute_read_sse_transcript(&absolute_target),
                    "text/event-stream",
                ),
        )
        .with_priority(2)
        .mount(&server)
        .await;

    let output = run_prompt_with_single_tool(
        temp.path(),
        &server,
        &["read"],
        "Read the absolute tool-target.txt path and summarize it.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "read");
    assert!(events_body.contains("tool-target.txt"));
    assert!(events_body.contains("alpha"));
}

async fn run_prompt_with_single_tool<const N: usize>(
    workspace_root: &std::path::Path,
    server: &MockServer,
    tools: &[&str; N],
    prompt_text: &str,
) -> std::process::Output {
    let config_path = workspace_root.join("harness.tool.jsonc");
    let session_dir = workspace_root.join("sessions");
    let out_path = workspace_root.join("events.jsonl");

    let config = prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, tools);

    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let out_arg = out_path.clone();
    let temp_path = workspace_root.to_path_buf();
    let prompt_text = prompt_text.to_string();
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_harness"))
            .current_dir(temp_path)
            .args([
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                &prompt_text,
                "--out",
                out_arg.to_str().expect("out path utf-8"),
            ])
            .output()
            .expect("run harness prompt")
    })
    .await
    .expect("join blocking command")
}

fn assert_successful_tool_roundtrip(
    output: &std::process::Output,
    events_body: &str,
    tool_id: &str,
) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(events_body.contains("\"event_type\":\"tool_call_requested\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_started\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"status\":\"succeeded\""));
    assert!(
        events_body.contains(tool_id),
        "expected events to mention {tool_id}: {events_body}"
    );
}

fn parse_prompt_events(events_body: &str) -> Vec<EventEnvelopeV1> {
    events_body
        .lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse prompt event"))
        .collect()
}

fn event_positions(
    events: &[EventEnvelopeV1],
    mut matches_event: impl FnMut(&EventEnvelopeV1) -> bool,
) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter_map(|(idx, event)| matches_event(event).then_some(idx))
        .collect()
}

fn first_event_position(
    events: &[EventEnvelopeV1],
    mut matches_event: impl FnMut(&EventEnvelopeV1) -> bool,
    description: &str,
) -> usize {
    events
        .iter()
        .position(|event| matches_event(event))
        .unwrap_or_else(|| panic!("missing {description} event"))
}

fn tool_request_position(events: &[EventEnvelopeV1], tool_id: &str, args_marker: &str) -> usize {
    first_event_position(
        events,
        |event| {
            matches!(&event.payload, EventV1::ToolCallRequested(payload)
                if payload.tool_id == tool_id && payload.args_summary.contains(args_marker))
        },
        &format!("{tool_id} tool request containing {args_marker}"),
    )
}

fn requested_tool_call_id(event: &EventEnvelopeV1) -> String {
    match &event.payload {
        EventV1::ToolCallRequested(payload) => payload.tool_call_id.clone(),
        _ => panic!("expected tool_call_requested event"),
    }
}

fn assert_tool_finished<'a>(
    events: &'a [EventEnvelopeV1],
    tool_call_id: &str,
    status: ToolCallStatus,
) -> &'a ToolCallFinishedEvent {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload)
                if payload.tool_call_id == tool_call_id && payload.status == status =>
            {
                Some(payload)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {status:?} finish for tool call {tool_call_id}"))
}

fn assert_request_has_function_call_output(request_body: &[u8], call_id: &str) {
    let body: serde_json::Value =
        serde_json::from_slice(request_body).expect("responses request body should parse as JSON");
    let input = body
        .get("input")
        .and_then(serde_json::Value::as_array)
        .expect("responses request should contain input array");
    assert!(
        input.iter().any(|item| {
            item.get("type") == Some(&serde_json::json!("function_call_output"))
                && item.get("call_id") == Some(&serde_json::json!(call_id))
        }),
        "responses request should include function_call_output for {call_id}: {body}"
    );
}

fn deterministic_responses_sse_transcript() -> String {
    [
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn title_responses_sse_transcript(title: &str) -> String {
    let delta = serde_json::json!({
        "type": "response.output_text.delta",
        "delta": title,
    });
    let completed = serde_json::json!({
        "type": "response.completed",
        "response": {
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 4,
                "total_tokens": 14,
            }
        }
    });
    format!(
        "event: response.output_text.delta\ndata: {delta}\n\nevent: response.completed\ndata: {completed}\n\ndata: [DONE]\n\n"
    )
}

fn reasoning_responses_sse_transcript() -> String {
    [
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Drafting a careful answer.\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn late_reasoning_duplicate_body_responses_sse_transcript() -> String {
    [
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi! How can I help?\"}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Responding to greetings\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"\\nHi! How can I help? \"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn repeated_body_chunks_before_reasoning_responses_sse_transcript() -> String {
    [
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"ha\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" ha\"}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"delta\":\"Done planning.\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_responses_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"tool-target.txt\\\",\\\"offset\\\":1\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_1\",\"delta\":\",\\\"limit\\\":20}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"tool-target.txt\\\",\\\"offset\\\":1,\\\"limit\\\":20}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_sse_transcript(
    item_id: &str,
    call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> String {
    let arguments = arguments.to_string();
    let item = serde_json::json!({
        "type": "function_call",
        "id": item_id,
        "call_id": call_id,
        "name": tool_name,
        "arguments": arguments,
    });
    let added = serde_json::json!({
        "type": "response.output_item.added",
        "item": item,
    })
    .to_string();
    let done = serde_json::json!({
        "type": "response.output_item.done",
        "item": item,
    })
    .to_string();

    [
        "event: response.output_item.added\n".to_string(),
        format!("data: {added}\n\n"),
        "event: response.output_item.done\n".to_string(),
        format!("data: {done}\n\n"),
        "event: response.completed\n".to_string(),
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":16,\"output_tokens\":4,\"total_tokens\":20}}}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat()
}

fn bash_artifact_call_responses_sse_transcript() -> String {
    let arguments = serde_json::json!({
        "command": "printf 'oracle%.0s' {1..11000}",
        "workdir": ".",
        "description": "emit deterministic oracle artifact",
        "timeout": 120000,
    })
    .to_string();
    let added = serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "function_call",
            "id": "item_oracle_bash",
            "call_id": "call_oracle_bash",
            "name": "bash",
            "arguments": arguments,
        }
    })
    .to_string();
    let done = serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "function_call",
            "id": "item_oracle_bash",
            "call_id": "call_oracle_bash",
            "name": "bash",
            "arguments": arguments,
        }
    })
    .to_string();

    [
        "event: response.output_item.added\n".to_string(),
        format!("data: {added}\n\n"),
        "event: response.output_item.done\n".to_string(),
        format!("data: {done}\n\n"),
        "event: response.completed\n".to_string(),
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":16,\"output_tokens\":4,\"total_tokens\":20}}}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat()
}

fn provider_error_sse_transcript() -> String {
    [
        "event: response.error\n",
        "data: {\"type\":\"response.error\",\"error\":{\"message\":\"fixture provider failure\"}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_missing_read_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_missing\",\"call_id\":\"call_missing\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"missing-tool-target.txt\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_missing\",\"delta\":\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_missing\",\"call_id\":\"call_missing\",\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"missing-tool-target.txt\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_result_followup_responses_sse_transcript() -> String {
    [
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Read complete: alpha beta gamma.\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":18,\"output_tokens\":6,\"total_tokens\":24}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_absolute_read_sse_transcript(path: &std::path::Path) -> String {
    let arguments = serde_json::json!({
        "path": path,
        "offset": 1,
        "limit": 20,
    })
    .to_string();
    let added = serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "function_call",
            "id": "item_1",
            "call_id": "call_1",
            "name": "read",
            "arguments": arguments,
        }
    })
    .to_string();
    let done = serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "type": "function_call",
            "id": "item_1",
            "call_id": "call_1",
            "name": "read",
            "arguments": arguments,
        }
    })
    .to_string();

    [
        "event: response.output_item.added\n".to_string(),
        format!("data: {added}\n\n"),
        "event: response.output_item.done\n".to_string(),
        format!("data: {done}\n\n"),
        "event: response.completed\n".to_string(),
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat()
}

fn tool_followup_text_sse_transcript(text: &str) -> String {
    [
        "event: response.output_text.delta\n",
        &format!(
            "data: {{\"type\":\"response.output_text.delta\",\"delta\":{}}}\n\n",
            serde_json::to_string(text).expect("serialize followup delta")
        ),
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":18,\"output_tokens\":6,\"total_tokens\":24}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_glob_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_glob\",\"call_id\":\"call_glob\",\"name\":\"glob\",\"arguments\":\"{\\\"pattern\\\":\\\"**/*.txt\\\",\\\"path\\\":\\\"fixtures\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_glob\",\"delta\":\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_glob\",\"call_id\":\"call_glob\",\"name\":\"glob\",\"arguments\":\"{\\\"pattern\\\":\\\"**/*.txt\\\",\\\"path\\\":\\\"fixtures\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_list_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_ls\",\"call_id\":\"call_ls\",\"name\":\"list\",\"arguments\":\"{\\\"path\\\":\\\"fixtures\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_ls\",\"delta\":\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_ls\",\"call_id\":\"call_ls\",\"name\":\"list\",\"arguments\":\"{\\\"path\\\":\\\"fixtures\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_grep_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_grep\",\"call_id\":\"call_grep\",\"name\":\"grep\",\"arguments\":\"{\\\"pattern\\\":\\\"BETA\\\",\\\"path\\\":\\\"fixtures\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_grep\",\"delta\":\",\\\"include\\\":\\\"*.md\\\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_grep\",\"call_id\":\"call_grep\",\"name\":\"grep\",\"arguments\":\"{\\\"pattern\\\":\\\"BETA\\\",\\\"path\\\":\\\"fixtures\\\",\\\"include\\\":\\\"*.md\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn write_resume_fixture_events(run_dir: &std::path::Path) {
    let events = [
        resume_envelope(
            "run_resume_cli",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "Original prompt".to_string(),
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            4,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-4o-mini".to_string(),
                prompt_summary: "Original prompt".to_string(),
                request_digest: "digest-original".to_string(),
                metadata: None,
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            5,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            6,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize resume fixture event"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
}

fn resume_envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Supervisor, Some("resume-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}
