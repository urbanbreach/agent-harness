use std::fs;
use std::process::Command;

use tempfile::tempdir;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

    let config = serde_json::json!({
        "backgroundTask": {
            "defaultConcurrency": 2,
            "providerConcurrency": 2,
            "modelConcurrency": 2,
            "staleTimeoutMs": 30000,
            "messageStalenessTimeoutMs": 10000
        },
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": format!("{}/v1", server.uri()),
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000
            }
        },
        "categories": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            }
        },
        "permissions": {
            "edit": "allow",
            "shell": "allow",
            "network": "allow"
        },
        "paths": {
            "session_dir": session_dir
        }
    })
    .to_string();

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

    let config = serde_json::json!({
        "backgroundTask": {
            "defaultConcurrency": 2,
            "providerConcurrency": 2,
            "modelConcurrency": 2,
            "staleTimeoutMs": 30000,
            "messageStalenessTimeoutMs": 10000
        },
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": format!("{}/v1", server.uri()),
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000
            }
        },
        "categories": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": ["fs.read"]
            }
        },
        "permissions": {
            "edit": "allow",
            "shell": "allow",
            "network": "allow"
        },
        "paths": {
            "session_dir": session_dir
        }
    })
    .to_string();

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

    let config = serde_json::json!({
        "backgroundTask": {
            "defaultConcurrency": 2,
            "providerConcurrency": 2,
            "modelConcurrency": 2,
            "staleTimeoutMs": 30000,
            "messageStalenessTimeoutMs": 10000
        },
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": format!("{}/v1", server.uri()),
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000
            }
        },
        "categories": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": tools
            }
        },
        "permissions": {
            "edit": "allow",
            "shell": "allow",
            "network": "allow"
        },
        "paths": {
            "session_dir": session_dir
        }
    })
    .to_string();

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
