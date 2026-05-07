use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent,
    TaskTerminalScope, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use tempfile::tempdir;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn prompt_cli_config(base_url: &str, session_dir: &std::path::Path, tools: &[&str]) -> String {
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

async fn run_prompt_with_single_tool(
    workspace_root: &std::path::Path,
    server: &MockServer,
    tools: &[&str],
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
