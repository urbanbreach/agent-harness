use std::fs;
use std::process::Command;

use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn prompt_cli_calls_responses_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_raw(deterministic_responses_sse_transcript(), "text/event-stream"),
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
