use harness::UnwrapOrAbort;
#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_executes_tool_call_and_completes_turn() {
    let provider = ScriptedPromptProvider::sequence(vec![
        tool_call_events(
            "call_1",
            "read",
            serde_json::json!({"path": "tool-target.txt", "offset": 1, "limit": 20}),
        ),
        text_events("Read complete: alpha beta gamma."),
    ]);

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.tool-loop.jsonc");
    let session_dir = temp.path().join("sessions");
    let out_path = temp.path().join("events.jsonl");
    fs::write(temp.path().join("tool-target.txt"), "alpha\nbeta\ngamma\n")
        .unwrap_or_abort();

    let config = prompt_cli_config("https://fixture.test/v1", &session_dir, &["read"]);

    fs::write(&config_path, config).unwrap_or_abort();

    let config_arg = config_path.clone();
    let out_arg = out_path.clone();
    let temp_path = temp.path().to_path_buf();
    let provider_for_run = provider.clone();
    let output = tokio::task::spawn_blocking(move || {
        run_harness_in_with_provider(temp_path, [
                "--config",
                config_arg.to_str().unwrap_or_abort(),
                "prompt",
                "--text",
                "Read tool-target.txt and then summarize it.",
                "--out",
                out_arg.to_str().unwrap_or_abort(),
            ], provider_for_run)
    })
    .await
    .unwrap_or_abort();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let events_body = fs::read_to_string(&out_path).unwrap_or_abort();
    assert!(
        events_body.contains("\"event_type\":\"tool_call_requested\""),
        "events:\n{events_body}"
    );
    assert!(events_body.contains("\"event_type\":\"tool_call_started\""));
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"status\":\"succeeded\""));
    assert!(events_body.contains("tool-target.txt"));
    assert!(
        events_body.contains("alpha")
            || events_body.contains("beta")
            || events_body.contains("gamma")
    );

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "expected tool loop to require two provider requests"
    );

    let messages = requests[1]
        .body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();

    let function_call_index = messages
        .iter()
        .position(|message| message.get("assistant_tool_calls").is_some())
        .unwrap_or_abort();
    let function_call_output_index = messages
        .iter()
        .position(|message| {
            message.get("role") == Some(&serde_json::Value::String("tool".to_string()))
        })
        .unwrap_or_abort();

    assert!(
        function_call_index < function_call_output_index,
        "function_call replay must appear before function_call_output: {}",
        requests[1].body
    );
    let assistant_tool_call = messages[function_call_index]
        .get("assistant_tool_calls")
        .and_then(serde_json::Value::as_array)
        .and_then(|calls| calls.first())
        .unwrap_or_abort();
    assert_eq!(
        assistant_tool_call.get("tool_call_id"),
        Some(&serde_json::Value::String("call_1".to_string()))
    );
    assert_eq!(messages[function_call_output_index].get("tool_call_id"), Some(&serde_json::Value::String("call_1".to_string())));

    let arguments = assistant_tool_call
        .get("arguments_json")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_abort();
    let parsed_arguments: serde_json::Value =
        serde_json::from_str(arguments).unwrap_or_abort();
    assert_eq!(
        parsed_arguments.get("path"),
        Some(&serde_json::Value::String("tool-target.txt".to_string()))
    );
}
#[tokio::test]
async fn prompt_cli_exits_nonzero_on_provider_error_finish() {
    let provider = ScriptedPromptProvider::fixed(provider_error_events());

    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.provider-error.jsonc");
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
            "Trigger a provider error.",
        ], provider)
        .await;

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
        .unwrap_or_abort()
        .map(|entry| entry.unwrap_or_abort().path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    assert_eq!(run_dirs.len(), 1, "expected one prompt run dir");
    let events_body =
        fs::read_to_string(run_dirs[0].join("events.jsonl")).unwrap_or_abort();
    assert!(events_body.contains("\"event_type\":\"provider_request_finished\""));
    assert!(events_body.contains("\"finish_reason\":\"error\""));
    assert!(events_body.contains("\"event_type\":\"task_cancelled\""));
}

#[tokio::test]
async fn prompt_cli_surfaces_provider_error_categories_in_stderr_and_events() {
    // arrange
    for category in [
        ProviderErrorCategory::MissingCredentials,
        ProviderErrorCategory::RateLimited,
        ProviderErrorCategory::ContextWindowExceeded,
    ] {
        let provider = ScriptedPromptProvider::fixed(categorized_provider_error_events(category));
        let temp = tempdir().unwrap_or_abort();
        let config_path = temp.path().join("harness.categorized-provider-error.jsonc");
        let session_dir = temp.path().join("sessions");
        fs::write(
            &config_path,
            prompt_cli_config("https://fixture.test/v1", &session_dir, &[]),
        )
        .unwrap_or_abort();

        let output = run_harness_in_blocking_with_provider(
            temp.path(),
            [
                "--config",
                config_path.to_str().unwrap_or_abort(),
                "prompt",
                "--text",
                "Trigger a categorized provider error.",
            ],
            provider,
        )
        .await;

        // act
        let stderr = String::from_utf8_lossy(&output.stderr);
        // assert
        assert!(!output.status.success(), "stderr:\n{stderr}");
        assert!(
            stderr.contains(category.as_str()),
            "stderr should render provider category {}:\n{stderr}",
            category.as_str()
        );
        assert!(
            stderr.contains("fixture provider failure"),
            "stderr should preserve provider message:\n{stderr}"
        );

        let run_dirs = fs::read_dir(&session_dir)
            .unwrap_or_abort()
            .map(|entry| entry.unwrap_or_abort().path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        assert_eq!(run_dirs.len(), 1, "expected one prompt run dir");
        let events_body = fs::read_to_string(run_dirs[0].join("events.jsonl"))
            .unwrap_or_abort();
        assert!(events_body.contains("\"finish_reason\":\"error\""));
        assert!(events_body.contains(&format!(
            "\"provider_error_category\":\"{}\"",
            category.as_str()
        )));
        assert!(events_body.contains("\"provider_error_remediation\""));
    }
}

#[allow(clippy::clone_on_ref_ptr, reason = "trait object coercion requires .clone() not Arc::clone")]
#[tokio::test]
async fn prompt_cli_continues_after_tool_failure_as_tool_message() {
    let provider = ScriptedPromptProvider::sequence(vec![
        tool_call_events(
            "call_missing",
            "read",
            serde_json::json!({"path": "missing-tool-target.txt"}),
        ),
        text_events("Recovered after the failed read tool call."),
    ]);

    let temp = tempdir().unwrap_or_abort();
    let output = run_prompt_with_single_tool(
        temp.path(),
        provider.clone(),
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

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).unwrap_or_abort();
    assert!(events_body.contains("\"event_type\":\"tool_call_finished\""));
    assert!(events_body.contains("\"status\":\"failed\""));
    assert!(events_body.contains("missing-tool-target.txt"));

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "tool failure should be returned as a tool message and followed by a second provider request"
    );
    let messages = requests[1]
        .body
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_abort();
    let tool_output = messages
        .iter()
        .find(|message| message.get("role") == Some(&serde_json::json!("tool")))
        .unwrap_or_abort();
    assert_eq!(
        tool_output.get("tool_call_id"),
        Some(&serde_json::json!("call_missing"))
    );
    assert!(
        tool_output
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_abort()
            .contains("tool call `read` failed"),
        "failed tool result should be sent back to the provider: {}",
        requests[1].body
    );
}
#[tokio::test]
async fn prompt_cli_executes_fs_glob_and_completes_turn() {
    let provider = ScriptedPromptProvider::sequence(vec![
        tool_call_events(
            "call_glob",
            "glob",
            serde_json::json!({"pattern": "**/*.txt", "path": "fixtures"}),
        ),
        text_events("Glob complete: fixtures/a.txt and fixtures/nested/b.txt."),
    ]);

    let temp = tempdir().unwrap_or_abort();
    fs::create_dir_all(temp.path().join("fixtures/nested")).unwrap_or_abort();
    fs::write(temp.path().join("fixtures/a.txt"), "alpha\n").unwrap_or_abort();
    fs::write(temp.path().join("fixtures/nested/b.txt"), "beta\n").unwrap_or_abort();
    fs::write(temp.path().join("fixtures/c.md"), "ignore\n").unwrap_or_abort();

    let output = run_prompt_with_single_tool(
        temp.path(),
        provider,
        &["glob"],
        "Use glob on fixtures and summarize the matches.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).unwrap_or_abort();
    assert_successful_tool_roundtrip(&output, &events_body, "glob");
    assert!(events_body.contains("fixtures/a.txt"));
    assert!(events_body.contains("fixtures/nested/b.txt"));
}
#[tokio::test]
async fn prompt_cli_executes_fs_ls_and_completes_turn() {
    let provider = ScriptedPromptProvider::sequence(vec![
        tool_call_events(
            "call_ls",
            "list",
            serde_json::json!({"path": "fixtures"}),
        ),
        text_events("Directory listing complete: alpha/, beta.txt, zeta.log."),
    ]);

    let temp = tempdir().unwrap_or_abort();
    fs::create_dir_all(temp.path().join("fixtures/alpha")).unwrap_or_abort();
    fs::write(temp.path().join("fixtures/beta.txt"), "beta\n").unwrap_or_abort();
    fs::write(temp.path().join("fixtures/zeta.log"), "zeta\n").unwrap_or_abort();

    let output = run_prompt_with_single_tool(
        temp.path(),
        provider,
        &["list"],
        "Use list on fixtures and summarize the entries.",
    )
    .await;

    let events_body = fs::read_to_string(temp.path().join("events.jsonl")).unwrap_or_abort();
    assert_successful_tool_roundtrip(&output, &events_body, "list");
    assert!(events_body.contains("alpha/"));
    assert!(events_body.contains("beta.txt"));
    assert!(events_body.contains("zeta.log"));
}
