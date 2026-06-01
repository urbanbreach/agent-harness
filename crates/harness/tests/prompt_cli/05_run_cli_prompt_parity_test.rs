#[test]
fn run_cli_mock_positional_message_prints_assistant_text() {
    let temp = tempdir().expect("tempdir");

    let output = run_harness_in(temp.path(), ["run", "--mock", "hello"]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Hello world"));
}

#[test]
fn run_cli_reads_piped_stdin_without_stdin_flag() {
    let temp = tempdir().expect("tempdir");
    let out_path = temp.path().join("events-pipe.jsonl");

    let output = run_harness_in_with_stdin(
        temp.path(),
        [
            "run",
            "--mock",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ],
        b"pipe\n".to_vec(),
    );

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(&out_path).expect("read events");
    assert!(events_body.contains("pipe"), "{events_body}");
}

#[test]
fn run_cli_combines_positional_message_and_piped_stdin() {
    let temp = tempdir().expect("tempdir");
    let out_path = temp.path().join("events-arg-pipe.jsonl");

    let output = run_harness_in_with_stdin(
        temp.path(),
        [
            "run",
            "--mock",
            "arg",
            "--out",
            out_path.to_str().expect("out path utf-8"),
        ],
        b"pipe\n".to_vec(),
    );

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(&out_path).expect("read events");
    assert!(events_body.contains("arg\\npipe"), "{events_body}");
}

#[test]
fn run_cli_no_input_on_tty_exits_quickly_with_clear_error() {
    let temp = tempdir().expect("tempdir");

    let output = run_harness_in(temp.path(), ["run", "--mock"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no prompt text provided"));
}

#[test]
fn run_cli_mock_model_and_agent_selector_fails_clearly_when_agent_is_unknown() {
    let temp = tempdir().expect("tempdir");

    let output = run_harness_in(
        temp.path(),
        [
            "run",
            "--mock",
            "-m",
            "mock:gpt-4o-mini",
            "--agent",
            "build",
            "hi",
        ],
    );

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown"), "stderr:\n{stderr}");
}

#[tokio::test]
async fn run_cli_explicit_session_resumes_prompt_session() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.resume.jsonc");
    let session_dir = temp.path().join("sessions");
    let resume_dir = session_dir.join("run_resume_cli");
    fs::create_dir_all(&resume_dir).expect("create resume run dir");
    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .expect("write config");
    write_resume_fixture_events(&resume_dir);

    let output = run_harness_in_blocking_with_provider(
        temp.path(),
        [
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "run",
            "--session",
            "run_resume_cli",
            "follow up",
        ],
        provider.clone(),
    )
    .await;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(resume_dir.join("events.jsonl")).expect("read events");
    assert!(events_body.contains("follow up"));
}

#[tokio::test]
async fn run_cli_continue_resumes_latest_resumable_session() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.continue.jsonc");
    let session_dir = temp.path().join("sessions");
    let resume_dir = session_dir.join("run_resume_cli");
    fs::create_dir_all(&resume_dir).expect("create resume run dir");
    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .expect("write config");
    write_resume_fixture_events(&resume_dir);

    let output = run_harness_in_blocking_with_provider(
        temp.path(),
        [
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "run",
            "-c",
            "follow up",
        ],
        provider,
    )
    .await;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(resume_dir.join("events.jsonl")).expect("read events");
    assert!(events_body.contains("follow up"));
}

#[test]
fn run_cli_json_format_emits_jsonl_only() {
    let temp = tempdir().expect("tempdir");

    let output = run_harness_in(temp.path(), ["run", "--format", "json", "--mock", "hi"]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        serde_json::from_str::<serde_json::Value>(line).expect("jsonl line");
    }
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Hello world\nHello"));
}

#[tokio::test]
async fn run_cli_file_flag_expands_text_file_context() {
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.file.jsonc");
    let session_dir = temp.path().join("sessions");
    fs::write(temp.path().join("notes.txt"), "alpha one\n").expect("write notes");
    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .expect("write config");

    let output = run_harness_in_blocking_with_provider(
        temp.path(),
        [
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "run",
            "-f",
            "notes.txt",
            "summarize",
        ],
        provider.clone(),
    )
    .await;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let body = provider.requests()[0].body.to_string();
    assert!(body.contains("notes.txt"));
    assert!(body.contains("1: alpha one"));
}

#[test]
fn run_cli_missing_file_fails_before_provider_call() {
    let temp = tempdir().expect("tempdir");

    let output = run_harness_in(temp.path(), ["run", "--mock", "-f", "missing.txt", "hi"]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--file path does not exist"));
}

#[test]
fn run_cli_first_slice_unsupported_flags_fail_clearly() {
    let temp = tempdir().expect("tempdir");

    let interactive = run_harness_in(temp.path(), ["run", "--interactive", "hi"]);
    assert!(!interactive.status.success());
    assert!(String::from_utf8_lossy(&interactive.stderr).contains("not implemented in this slice"));

    let command = run_harness_in(temp.path(), ["run", "--command", "foo", "bar"]);
    assert!(!command.status.success());
    assert!(String::from_utf8_lossy(&command.stderr).contains("not implemented in this slice"));
}
