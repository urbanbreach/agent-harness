use std::fs;
use std::process::Command;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
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
                        "display_name": "GPT-4o mini"
                    }
                }
            }
        },
        "profiles": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
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
        .expect("write tool target");

    let config = prompt_cli_config(&format!("{}/v1", server.uri()), &session_dir, &["fs.read"]);

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
            "Use fs.glob on fixtures and summarize the matches.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_fs_glob_sse_transcript(), "text/event-stream"),
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
        &["fs.glob"],
        "Use fs.glob on fixtures and summarize the matches.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "fs.glob");
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
            "Use fs.ls on fixtures and summarize the entries.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_fs_ls_sse_transcript(), "text/event-stream"),
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
        &["fs.ls"],
        "Use fs.ls on fixtures and summarize the entries.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "fs.ls");
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
            "Use fs.grep in fixtures for BETA and summarize the hit.",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(tool_call_fs_grep_sse_transcript(), "text/event-stream"),
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
        &["fs.grep"],
        "Use fs.grep in fixtures for BETA and summarize the hit.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).expect("read events");
    assert_successful_tool_roundtrip(&output, &events_body, "fs.grep");
    assert!(events_body.contains("fixtures/notes.md:2: BETA match"));
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

fn tool_call_responses_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"fs_read\",\"arguments\":\"{\\\"path\\\":\\\"tool-target.txt\\\",\\\"offset\\\":1\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_1\",\"delta\":\",\\\"limit\\\":20,\\\"line_numbers\\\":true}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_1\",\"call_id\":\"call_1\",\"name\":\"fs_read\",\"arguments\":\"{\\\"path\\\":\\\"tool-target.txt\\\",\\\"offset\\\":1,\\\"limit\\\":20,\\\"line_numbers\\\":true}\"}}\n\n",
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

fn tool_call_fs_glob_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_glob\",\"call_id\":\"call_glob\",\"name\":\"fs_glob\",\"arguments\":\"{\\\"pattern\\\":\\\"**/*.txt\\\",\\\"path\\\":\\\"fixtures\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_glob\",\"delta\":\",\\\"limit\\\":10}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_glob\",\"call_id\":\"call_glob\",\"name\":\"fs_glob\",\"arguments\":\"{\\\"pattern\\\":\\\"**/*.txt\\\",\\\"path\\\":\\\"fixtures\\\",\\\"limit\\\":10}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_fs_ls_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_ls\",\"call_id\":\"call_ls\",\"name\":\"fs_ls\",\"arguments\":\"{\\\"path\\\":\\\"fixtures\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_ls\",\"delta\":\",\\\"limit\\\":10}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_ls\",\"call_id\":\"call_ls\",\"name\":\"fs_ls\",\"arguments\":\"{\\\"path\\\":\\\"fixtures\\\",\\\"limit\\\":10}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":12,\"output_tokens\":3,\"total_tokens\":15}}}\n\n",
        "data: [DONE]\n\n",
    ]
    .concat()
}

fn tool_call_fs_grep_sse_transcript() -> String {
    [
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"item_grep\",\"call_id\":\"call_grep\",\"name\":\"fs_grep\",\"arguments\":\"{\\\"pattern\\\":\\\"BETA\\\",\\\"path\\\":\\\"fixtures\\\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"item_grep\",\"delta\":\",\\\"include\\\":\\\"*.md\\\",\\\"limit\\\":10,\\\"context\\\":0}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"item_grep\",\"call_id\":\"call_grep\",\"name\":\"fs_grep\",\"arguments\":\"{\\\"pattern\\\":\\\"BETA\\\",\\\"path\\\":\\\"fixtures\\\",\\\"include\\\":\\\"*.md\\\",\\\"limit\\\":10,\\\"context\\\":0}\"}}\n\n",
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
            }),
        ),
        resume_envelope(
            "run_resume_cli",
            5,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
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
