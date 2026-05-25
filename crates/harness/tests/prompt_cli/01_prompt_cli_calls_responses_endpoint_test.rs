#[tokio::test]
async fn prompt_cli_calls_responses_endpoint() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.test.jsonc");
    let session_dir = temp.path().join("sessions");

    let config = prompt_cli_config("https://fixture.test/v1", &session_dir, &[]);

    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let provider_for_run = provider.clone();
    let output = tokio::task::spawn_blocking(move || {
        run_harness_in_with_provider(temp_path, [
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Hello",
            ], provider_for_run)
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert_eq!(provider.requests().len(), 1);
}
#[tokio::test]
async fn prompt_cli_expands_at_file_and_directory_tags_for_provider() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.test.jsonc");
    let session_dir = temp.path().join("sessions");
    fs::write(temp.path().join("alpha.txt"), "alpha one\nalpha two\n").expect("write file");
    fs::create_dir(temp.path().join("src")).expect("create src");
    fs::write(temp.path().join("src/lib.rs"), "pub fn demo() {}\n").expect("write nested file");

    let config = prompt_cli_config("https://fixture.test/v1", &session_dir, &[]);
    fs::write(&config_path, config).expect("write config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let provider_for_run = provider.clone();
    let output = tokio::task::spawn_blocking(move || {
        run_harness_in_with_provider(temp_path, [
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Summarize @alpha.txt and list @src",
            ], provider_for_run)
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let body = provider.requests()[0].body.to_string();

    assert!(body.contains("Summarize @alpha.txt and list @src"));
    assert!(body.contains("Called the Read tool with the following input"));
    assert!(body.contains("alpha.txt"));
    assert!(body.contains("1: alpha one"));
    assert!(body.contains("2: alpha two"));
    assert!(body.contains("lib.rs"));
}
#[tokio::test]
async fn prompt_cli_generates_harness_session_title() {
    let provider = ScriptedPromptProvider::fixed(text_events("Debugging production 500 errors"));

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.public.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");
    fs::write(
        &config_path,
        prompt_cli_public_runtime_config("https://fixture.test/v1"),
    )
    .expect("write config");

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "prompt",
            "--text",
            "debug 500 errors in production",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ], provider.clone())
        .await;

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

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "expected title request plus main prompt request"
    );
    let first_request_body = requests[0].body.to_string();
    assert!(
        first_request_body.contains("Generate a title for this conversation:"),
        "first provider request should be the harness title request: {first_request_body}"
    );
}
#[tokio::test]
async fn prompt_tracker_waits_for_agent_turn_end_not_provider_finish() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.prompt-tracker.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");

    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .expect("write config");

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "Hello",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ], provider)
        .await;

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
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.public.jsonc");

    fs::write(
        &config_path,
        prompt_cli_public_runtime_config("https://fixture.test/v1"),
    )
    .expect("write public config");

    let config_arg = config_path.clone();
    let temp_path = temp.path().to_path_buf();
    let provider_for_run = provider.clone();
    let output = tokio::task::spawn_blocking(move || {
        run_harness_in_with_provider(temp_path, [
                "--config",
                config_arg.to_str().expect("config path utf-8"),
                "prompt",
                "--text",
                "Hello",
            ], provider_for_run)
    })
    .await
    .expect("join blocking command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let requests = provider.requests();
    let main_request = requests
        .iter()
        .find(|request| request.body.to_string().contains("Hello"))
        .expect("expected captured main prompt request");
    assert_eq!(main_request.provider_id.as_deref(), Some("default"));
    assert_eq!(main_request.model_id, "gpt-5.4-mini");
}
#[tokio::test]
async fn prompt_cli_creates_durable_run_logs_under_run_dir() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.logging.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .expect("write config");

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "prompt",
            "--text",
            "Hello",
            "--print-run-dir",
        ], provider)
        .await;

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
