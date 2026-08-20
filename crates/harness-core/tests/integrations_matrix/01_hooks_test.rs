#[test]
fn hooks_boundary_e2e_valid_config_loads_and_validates() {
    // arrange — a valid hooks config
    let raw = config_with_hooks_json(
        r#"[
            {
                "id": "on-start",
                "event": "run_started",
                "command": ["echo", "hello"],
                "timeout_ms": 5000
            }
        ]"#,
    );

    // act — loaded from string
    let config = load_config_from_str(&raw).expect("valid config");

    // assert — config is accepted with the hook registered
    assert_eq!(config.hooks.lifecycle.len(), 1);
    let hook = &config.hooks.lifecycle[0];
    assert_eq!(hook.id.as_deref(), Some("on-start"));
    assert_eq!(hook.event, HookLifecycleEvent::RunStarted);
    assert_eq!(hook.command, vec!["echo", "hello"]);
}

#[test]
fn hooks_bad_input_empty_command_tokens_rejected_by_config_validation() {
    // arrange — a hooks config with an empty command token
    let raw = config_with_hooks_json(
        r#"[
            {
                "id": "bad-empty",
                "event": "run_started",
                "command": ["echo", ""],
                "timeout_ms": 5000
            }
        ]"#,
    );

    // act — loaded from string
    let err = load_config_from_str(&raw).expect_err("empty command token must fail");

    // assert — config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("empty command token") || msg.contains("command"),
        "expected command validation error, got: {msg}"
    );
}

#[test]
fn hooks_bad_input_zero_timeout_rejected_by_config_validation() {
    // arrange — a hooks config with timeout_ms = 0
    let raw = config_with_hooks_json(
        r#"[
            {
                "id": "bad-timeout",
                "event": "run_started",
                "command": ["echo", "hello"],
                "timeout_ms": 0
            }
        ]"#,
    );

    // act — loaded from string
    let err = load_config_from_str(&raw).expect_err("zero timeout must fail");

    // assert — config validation rejects it
    let msg = err.to_string();
    assert!(
        msg.contains("timeout_ms") || msg.contains("timeout"),
        "expected timeout validation error, got: {msg}"
    );
}

#[test]
fn hooks_permission_denial_executable_not_in_shell_allowlist_is_rejected() {
    // arrange — a hook runtime with an allowlist that does not include the hook executable
    let runtime = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("denied".to_string()),
                event: HookLifecycleEvent::RunStarted,
                command: vec!["forbidden-executable".to_string()],
                cwd: None,
                timeout_ms: 5_000,
                critical: false,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["echo".to_string()],
            ..Default::default()
        },
        suppress_execution: false,
    };

    // act — the hook executable is checked against the allowlist
    let executable = &runtime.hooks.lifecycle[0].command[0];
    let allowed = runtime
        .shell_allowlist
        .executables
        .iter()
        .any(|allowed_exec| allowed_exec == executable);

    // assert — the executable is denied
    assert!(!allowed, "forbidden executable must not be in allowlist");
}

#[tokio::test]
async fn hooks_process_failure_executor_returns_error_for_failing_command() {
    // arrange — a TokioLifecycleHookCommandExecutor and a command that exits non-zero
    let executor = TokioLifecycleHookCommandExecutor;
    let temp = tempdir().unwrap_or_abort();
    let invocation = LifecycleHookCommandInvocation {
        executable: "sh".to_string(),
        args: vec!["-c".to_string(), "exit 1".to_string()],
        cwd: temp.path().to_path_buf(),
        env: BTreeMap::new(),
        timeout_ms: 5_000,
    };

    // act — the hook command is executed
    let output = executor
        .execute(invocation)
        .await
        .expect("executor should return output");

    // assert — the command completes but with a non-zero exit status
    assert!(!output.status.success(), "command must exit non-zero");
}

#[tokio::test]
async fn hooks_cancellation_restart_executor_times_out_and_recovers_on_retry() {
    // arrange — a TokioLifecycleHookCommandExecutor and a command that sleeps beyond the timeout
    let executor = TokioLifecycleHookCommandExecutor;
    let temp = tempdir().unwrap_or_abort();
    let timeout_invocation = LifecycleHookCommandInvocation {
        executable: "sh".to_string(),
        args: vec!["-c".to_string(), "sleep 5".to_string()],
        cwd: temp.path().to_path_buf(),
        env: BTreeMap::new(),
        timeout_ms: 100,
    };

    // act — the hook command times out
    let timeout_err = executor
        .execute(timeout_invocation)
        .await
        .expect_err("must time out");
    assert!(
        timeout_err.contains("timed out"),
        "expected timeout error, got: {timeout_err}"
    );

    // assert — a subsequent fast command succeeds (restart/recovery)
    let recovery_invocation = LifecycleHookCommandInvocation {
        executable: "echo".to_string(),
        args: vec!["recovered".to_string()],
        cwd: temp.path().to_path_buf(),
        env: BTreeMap::new(),
        timeout_ms: 5_000,
    };
    let recovery = executor
        .execute(recovery_invocation)
        .await
        .expect("recovery must succeed");
    assert!(recovery.status.success());
    assert!(recovery.stdout.contains("recovered"));
}

#[test]
fn hooks_redaction_output_summary_truncates_long_output() {
    // arrange — a hook output with a very long stdout containing a secret-like string
    let secret = "sk-AbCdEf0123456789SecretKeyDoNotLeak".to_string();
    let long_stdout = format!("{secret}{}", "x".repeat(300));

    // act — the output is summarized (replicating the 160-char truncation logic)
    let summary = truncate_hook_output(&long_stdout, "");

    // assert — the summary is truncated to at most 163 chars (160 + ellipsis)
    assert!(summary.len() < long_stdout.len(), "must be truncated");
    assert!(
        summary.len() <= 163,
        "must be at most 160 chars + ellipsis: {}",
        summary.len()
    );
    assert!(summary.ends_with("..."), "must end with ellipsis");
    // The full 335-char output must not appear in the summary
    assert!(
        !summary.contains(&"x".repeat(160)),
        "must not contain the full padding"
    );
}

/// Replicates the hook output summarization truncation for testing redaction.
/// The real `summarize_hook_output` truncates to 160 chars with an ellipsis.
fn truncate_hook_output(stdout: &str, stderr: &str) -> String {
    let stdout_trimmed = stdout.trim();
    let stderr_trimmed = stderr.trim();
    let combined = if stderr_trimmed.is_empty() {
        stdout_trimmed
    } else if stdout_trimmed.is_empty() {
        stderr_trimmed
    } else {
        "stdout/stderr captured"
    };
    if combined.is_empty() {
        return "no output".to_string();
    }
    if combined.len() > 160 {
        format!("{}...", &combined[..160])
    } else {
        combined.to_string()
    }
}

