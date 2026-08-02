use super::*;
use harness::UnwrapOrAbort;

#[test]
fn tui_auth_backend_runs_same_auth_command_and_redacts_output() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let config_path = temp.path().join("harness.jsonc");
    std::fs::write(
        &config_path,
        r#"
        {
          provider: {
            codex_route: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              authProvider: "codex",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "codex_route/gpt-5.4-mini",
          permission: "ask",
        }
        "#,
    )
    .unwrap_or_abort();
    let deps = harness::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());

    let secret = "tui-auth-backend-secret-value";
    let (message, level) = run_tui_auth_backend_once_with_deps(
        vec![
            "login".to_string(),
            "codex".to_string(),
            "--mock-token".to_string(),
            secret.to_string(),
        ],
        Some(config_path.clone()),
        Some(temp.path().join("sessions")),
        &deps,
    );

    assert_eq!(level, OperatorNoticeLevel::Info);
    assert!(message.contains("auth backend completed: harness auth login codex"));
    assert!(!message.contains(secret), "TUI notice leaked auth secret");
    assert!(
        data_home.join("harness/credentials/codex.json").is_file(),
        "TUI auth route must write through the same credential backend as CLI auth"
    );

    let (list_message, list_level) = run_tui_auth_backend_once_with_deps(
        vec!["list".to_string()],
        Some(config_path),
        Some(temp.path().join("sessions")),
        &deps,
    );
    assert_eq!(list_level, OperatorNoticeLevel::Info);
    assert!(list_message.contains("presence=stored"));
    assert!(
        !list_message.contains(secret),
        "TUI auth list leaked auth secret"
    );
}

#[test]
fn tui_auth_backend_streams_output_and_accepts_hidden_stdin() {
    // arrange
    // act
    // assert
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let config_path = temp.path().join("harness.jsonc");
    std::fs::write(
        &config_path,
        r#"
        {
          provider: {
            codex_route: {
              type: "openai_compatible",
              baseURL: "http://127.0.0.1:8317/v1",
              authProvider: "codex",
              models: {
                "gpt-5.4-mini": { name: "GPT-5.4 mini" },
              },
            },
          },
          model: "codex_route/gpt-5.4-mini",
          permission: "ask",
        }
        "#,
    )
    .unwrap_or_abort();
    let deps = harness::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());
    let secret = "sk-tui-streamed-stdin-secret";
    let (tx, rx) = std_mpsc::channel();

    let (message, level, success) = run_tui_auth_backend_streaming_with_deps(
        vec![
            "login".to_string(),
            "codex".to_string(),
            "--method".to_string(),
            "api-key".to_string(),
            "--api-key-stdin".to_string(),
        ],
        Some(config_path),
        Some(temp.path().join("sessions")),
        secret,
        &deps,
        Some(tx),
    );

    assert!(success);
    assert_eq!(level, OperatorNoticeLevel::Info);
    assert!(
        !message.contains(secret),
        "final notice leaked stdin secret"
    );
    let notices = rx.try_iter().collect::<Vec<_>>();
    assert!(
        notices.iter().any(|update| matches!(
            update,
            LiveUpdate::OperatorNotice { message, level: OperatorNoticeLevel::Info }
                if message.contains("stored api_key credential for codex")
        )),
        "expected streamed auth output before the final completion notice"
    );
    assert!(
        notices.iter().all(|update| match update {
            LiveUpdate::OperatorNotice { message, .. } => !message.contains(secret),
            _ => true,
        }),
        "streamed auth notice leaked stdin secret"
    );
    assert!(
        data_home.join("harness/credentials/codex.json").is_file(),
        "streamed TUI auth should store the API key through the CLI backend"
    );
}

#[test]
fn tui_auth_backend_preserves_browser_bind_error() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let deps = harness::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());
    let listener = std::net::TcpListener::bind(("127.0.0.1", 1455)).unwrap_or_abort();

    // act
    let (message, level) = run_tui_auth_backend_once_with_deps(
        vec![
            "login".to_string(),
            "openai".to_string(),
            "--method".to_string(),
            "browser".to_string(),
        ],
        None,
        Some(temp.path().join("sessions")),
        &deps,
    );

    // assert
    drop(listener);
    assert_eq!(level, OperatorNoticeLevel::Error);
    assert!(
        message.contains("could not bind Codex loopback callback"),
        "browser bind failure must remain visible in the TUI result: {message}"
    );
}

#[test]
fn tui_auth_backend_streams_browser_bind_error_detail() {
    // arrange
    let temp = tempfile::tempdir().unwrap_or_abort();
    let data_home = temp.path().join("data");
    let deps = harness::CliDeps::real()
        .with_current_dir(temp.path().to_path_buf())
        .with_env("HARNESS_DATA_HOME", data_home.to_string_lossy());
    let listener = std::net::TcpListener::bind(("127.0.0.1", 1455)).unwrap_or_abort();
    let (tx, rx) = std_mpsc::channel();

    // act
    let (message, level, success) = run_tui_auth_backend_streaming_with_deps(
        vec![
            "login".to_string(),
            "openai".to_string(),
            "--method".to_string(),
            "browser".to_string(),
        ],
        None,
        Some(temp.path().join("sessions")),
        "",
        &deps,
        Some(tx),
    );

    // assert
    drop(listener);
    assert_eq!(level, OperatorNoticeLevel::Error);
    assert!(!success);
    assert!(message.contains("could not bind Codex loopback callback"));
    assert!(rx.try_iter().any(|update| matches!(
        update,
        LiveUpdate::OperatorNotice { message, level: OperatorNoticeLevel::Error }
            if message.contains("could not bind Codex loopback callback")
    )));
}
