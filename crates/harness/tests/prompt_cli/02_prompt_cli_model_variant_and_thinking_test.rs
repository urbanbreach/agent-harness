use harness::UnwrapOrAbort;
#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_model_variant_and_thinking_flags_stream_reasoning_output() {
    // arrange
    // act
    // assert
    let provider = ScriptedPromptProvider::fixed(reasoning_events());

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.reasoning.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .unwrap_or_abort();

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "prompt",
            "--text",
            "Hello",
            "--model",
            "default:gpt-4o-mini",
            "--variant",
            "low",
            "--thinking",
        ], provider.clone())
        .await;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Thinking: Drafting a careful answer."));
    assert!(stdout.contains("Hello world"));
    let requests = provider.requests();
    assert_eq!(requests[0].reasoning_effort.as_deref(), Some("low"));
    assert_eq!(requests[0].reasoning_summary.as_deref(), Some("auto"));
    assert_eq!(requests[0].text_verbosity.as_deref(), Some("low"));
}
#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_model_override_records_selected_model_in_run_metadata() {
    // arrange
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.model-override.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        serde_json::json!({
            "providers": {
                "default": {
                    "type": "openai_compatible",
                    "base_url": "https://fixture.test/v1",
                    "api_key": "DUMMY",
                    "api_mode": "responses",
                    "timeout_ms": 60000,
                    "models": {
                        "gpt-4o-mini": {
                            "display_name": "GPT-4o mini"
                        },
                        "gpt-4.1": {
                            "display_name": "GPT-4.1"
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
            "ui": {
                "default_profile": "deep"
            }
        })
        .to_string(),
    )
    .unwrap_or_abort();

    // act
    let output = run_harness_in_blocking_with_provider(
        temp.path(),
        [
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "prompt",
            "--text",
            "Hello",
            "--model",
            "default:gpt-4.1",
            "--print-run-dir",
        ],
        provider.clone(),
    )
    .await;

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let run_dir = stdout
        .lines()
        .last()
        .unwrap_or_abort();
    let metadata_body = fs::read_to_string(std::path::Path::new(run_dir).join("meta.json"))
        .unwrap_or_abort();
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_body).unwrap_or_abort();
    assert_eq!(
        metadata["recorded_runtime_context"]["model"].as_str(),
        Some("gpt-4.1")
    );

    let requests = provider.requests();
    assert_eq!(requests[0].model_id, "gpt-4.1");
}
#[tokio::test]
async fn prompt_cli_thinking_prints_late_reasoning_before_one_assistant_body() {
    // arrange
    // act
    // assert
    let provider = ScriptedPromptProvider::fixed(late_reasoning_duplicate_body_events());

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.late-reasoning.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .unwrap_or_abort();

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "prompt",
            "--text",
            "hi",
            "--thinking",
        ], provider)
        .await;

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let thinking = "Thinking: Responding to greetings";
    let body = "Hi! How can I help?";
    let thinking_index = stdout.find(thinking).unwrap_or_abort();
    let body_index = stdout.find(body).unwrap_or_abort();
    assert!(
        thinking_index < body_index,
        "thinking should print before assistant body:\n{stdout}"
    );
    assert_eq!(stdout.matches(body).count(), 1, "{stdout}");
}
#[tokio::test]
async fn prompt_cli_thinking_preserves_repeated_body_chunks_before_reasoning() {
    // arrange
    // act
    // assert
    let provider = ScriptedPromptProvider::fixed(repeated_body_chunks_before_reasoning_events());

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.repeated-chunks.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .unwrap_or_abort();

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "prompt",
            "--text",
            "repeat",
            "--thinking",
        ], provider)
        .await;

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
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.models.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(
        &config_path,
        prompt_cli_config("http://127.0.0.1:9999/v1", &session_dir, &[]),
    )
    .unwrap_or_abort();

    let output = run_harness_in(temp.path(), [
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "models",
        ]);

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
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let out_path = temp.path().join("events.jsonl");

    let output = run_harness_in(temp.path(), [
            "prompt",
            "--mock",
            "--text",
            "Hello from PTY",
            "--out",
            out_path.to_str().unwrap_or_abort(),
        ]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&out_path).unwrap_or_abort();
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
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let out_path = temp.path().join("events-positional.jsonl");

    let output = run_harness_in(temp.path(), [
            "prompt",
            "--mock",
            "Hello from PTY",
            "--out",
            out_path.to_str().unwrap_or_abort(),
        ]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(&out_path).unwrap_or_abort();
    assert!(events_body.contains("Hello world"));
}
#[test]
fn prompt_cli_mock_mode_accepts_stdin_text() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let out_path = temp.path().join("events-stdin.jsonl");

    let output = run_harness_in_with_stdin(
        temp.path(),
        [
            "prompt",
            "--mock",
            "--stdin",
            "--out",
            out_path.to_str().unwrap_or_abort(),
        ],
        b"Hello from PTY\n".to_vec(),
    );
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let events_body = fs::read_to_string(&out_path).unwrap_or_abort();
    assert!(events_body.contains("Hello world"));
}
#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_uses_merged_xdg_and_local_config_without_explicit_path() {
    // arrange
    // act
    // assert
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().unwrap_or_abort();
    let xdg_root = temp.path().join("xdg");
    let xdg_config_path = xdg_root.join("harness/harness.jsonc");
    let local_config_path = temp.path().join("harness.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::create_dir_all(xdg_config_path.parent().unwrap_or_abort())
        .unwrap_or_abort();
    fs::write(
        &xdg_config_path,
        serde_json::json!({
            "providers": {
                "default": {
                    "type": "openai_compatible",
                            "base_url": "https://fixture.test/v1",
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
    .unwrap_or_abort();
    fs::write(
        &local_config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .unwrap_or_abort();

    let output = run_harness_in_blocking_with_provider(
        temp.path(),
        [
            "--config",
            local_config_path.to_str().unwrap_or_abort(),
            "prompt",
            "Hello from merged config",
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

    assert_eq!(provider.requests().len(), 1);
}
#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_resume_flag_continues_existing_session() {
    // arrange
    // act
    // assert
    let provider = ScriptedPromptProvider::fixed(text_events("Hello"));

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.resume.jsonc");
    let session_dir = temp.path().join("sessions");
    let resume_dir = session_dir.join("run_resume_cli");
    fs::create_dir_all(&resume_dir).unwrap_or_abort();
    fs::create_dir_all(temp.path().join("workspace")).unwrap_or_abort();
    fs::write(
        &config_path,
        prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
    )
    .unwrap_or_abort();
    write_resume_fixture_events(&resume_dir);

    let output = run_harness_in_blocking_with_provider(temp.path(), [
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "prompt",
            "--resume",
            "run_resume_cli",
            "--text",
            "Continue from the saved session.",
            "--print-run-dir",
        ], provider.clone())
        .await;

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
        fs::read_to_string(resume_dir.join("events.jsonl")).unwrap_or_abort();
    assert!(events_body.contains("\"request_id\":\"req_000002\""));
    assert!(events_body.contains("Continue from the saved session."));

    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");
    assert!(requests[0].body.to_string().contains("Continue from the saved session."));
}
