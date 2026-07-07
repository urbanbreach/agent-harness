use harness::UnwrapOrAbort;
#[test]
fn replay_bootstrap_falls_back_when_recorded_runtime_context_missing() {
    let run_dir = tempdir().unwrap_or_abort();
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "legacy-profile".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope_with_correlation(
                3,
                Some("req_000001"),
                EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "legacy-provider".to_string(),
                    model_id: "legacy-model".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    std::fs::write(
        run_dir.path().join("meta.json"),
        serde_json::json!({
            "run_id": "run_fixture",
            "run_name": "interactive",
            "workspace_root": "/tmp/workspace",
            "config_digest": "none",
            "harness_version": env!("CARGO_PKG_VERSION")
        })
        .to_string(),
    )
    .unwrap_or_abort();

    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();
    let launch_metadata = tui_impl::replay_launch_metadata_for_test(run_dir.path(), &events);

    assert_eq!(launch_metadata.profile(), "legacy-profile");
    assert_eq!(launch_metadata.provider(), "legacy-provider");
    assert_eq!(launch_metadata.model(), Some("legacy-model"));
    assert_eq!(launch_metadata.mode_label(), Some("Replay"));
}
#[test]
fn replay_bootstrap_prefers_recorded_runtime_context_from_meta() {
    let run_dir = tempdir().unwrap_or_abort();
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "legacy-profile".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope_with_correlation(
                3,
                Some("req_000001"),
                EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "legacy-provider".to_string(),
                    model_id: "legacy-model".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
        ],
    );
    std::fs::write(
        run_dir.path().join("meta.json"),
        serde_json::json!({
            "run_id": "run_fixture",
            "run_name": "interactive",
            "workspace_root": "/tmp/workspace",
            "config_digest": "none",
            "harness_version": env!("CARGO_PKG_VERSION"),
            "recorded_runtime_context": {
                "profile": "archive",
                "provider": "default",
                "model": "gpt-5.4-mini",
                "variant": "deterministic",
                "display_label": "GPT-5.4 Mini · Deterministic"
            }
        })
        .to_string(),
    )
    .unwrap_or_abort();

    let events = load_events_from_run_dir(run_dir.path()).unwrap_or_abort();
    let launch_metadata = tui_impl::replay_launch_metadata_for_test(run_dir.path(), &events);

    assert_eq!(launch_metadata.profile(), "archive");
    assert_eq!(launch_metadata.provider(), "default");
    assert_eq!(launch_metadata.model(), Some("gpt-5.4-mini"));
    assert_eq!(launch_metadata.variant(), Some("deterministic"));
    assert_eq!(
        launch_metadata.display_label(),
        Some("GPT-5.4 Mini · Deterministic")
    );
    assert_eq!(launch_metadata.mode_label(), Some("Replay"));
}
#[test]
fn tui_replay_and_continue_headers_are_distinct() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();

    let events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];

    set_pending_live_launch_metadata(
        LaunchMetadata::new("alpha", "mock", Some("model-1".to_string()))
            .with_mode_label("Continued"),
    );
    let mut continued = AppState::new_live(
        Some(std::path::PathBuf::from("/tmp/sessions/run_continue")),
        false,
        None,
    );
    continued.replace_events(events.clone());

    let replay = AppState::new_replay(
        std::path::PathBuf::from("/tmp/sessions/run_continue"),
        events,
    );

    assert_eq!(continued.launch_mode_label(), Some("Continued"));
    assert!(!continued.replay_mode);
    assert!(replay.replay_mode);
    assert!(
        replay.runtime_state().summary.contains("events loaded"),
        "replay runtime should stay read-only and distinct from continued live mode"
    );
}
#[test]
fn tui_cli_without_config_reaches_connect_startup() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    let temp = tempdir().unwrap_or_abort();
    let output = run_harness_in(temp.path(), ["tui", "--exit-on-finish"]);

    assert_no_config_startup_exits_cleanly("no-config interactive startup", &output);
}
#[test]
fn tui_cli_explicit_launch_reuses_no_config_startup() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    let temp = tempdir().unwrap_or_abort();
    let output = run_harness_in(temp.path(), ["tui", "--exit-on-finish"]);

    assert_no_config_startup_exits_cleanly("explicit tui launch", &output);
}
#[test]
fn tui_cli_legacy_tui_alias_reuses_no_config_startup() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    let temp = tempdir().unwrap_or_abort();
    let output = run_harness_in(temp.path(), ["tui", "--exit-on-finish"]);

    assert_no_config_startup_exits_cleanly("legacy tui alias", &output);
}

fn assert_no_config_startup_exits_cleanly(context: &str, output: &CliHarnessOutput) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("tui setup failed:"),
        "{context} should use the built-in connect state without config, got:\n{stderr}"
    );
    assert!(
        output.status.success()
            || stderr.contains("startup launcher error:")
            || stderr.contains("failed to enable terminal raw mode")
            || stderr.contains("tui failed: TUI error:"),
        "{context} should exit successfully with --exit-on-finish or reach a terminal startup boundary, got stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
}
#[test]
fn tui_cli_mock_flag_starts_demo_mode() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    let temp = tempdir().unwrap_or_abort();
    let output = run_harness_in(temp.path(), ["tui", "--mock", "--exit-on-finish"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("tui setup failed:"),
        "expected --mock to bypass config guidance, got:\n{stderr}"
    );
    assert!(
        output.status.success()
            || stderr.contains("tui failed: TUI error:")
            || stderr.contains("tui failed: startup launcher error:"),
        "expected --mock to reach demo mode startup, got stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr,
    );
}
#[test]
fn tui_mock_mode_still_boots_through_launcher() {
    let _guard = startup_draft_test_lock()
        .lock()
        .unwrap_or_abort();
    let temp = tempdir().unwrap_or_abort();
    let output = run_harness_in(temp.path(), ["tui", "--mock", "--exit-on-finish"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("tui setup failed:"),
        "expected --mock to bypass config guidance, got:\n{stderr}"
    );
    assert!(
        output.status.success() || stderr.contains("failed to enable terminal raw mode"),
        "expected --mock to reach the interactive TUI boundary, got stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr,
    );
}
#[test]
fn tui_cli_root_help_only_shows_minimal_interactive_overrides() {
    let output = run_harness(["--help"]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Launch the interactive harness UI or run subcommands"));
    assert!(stdout.contains("--profile <PROFILE>"));
    assert!(stdout.contains("--mock"));
    assert!(stdout.contains("tui"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("prompt"));
    assert!(
        !stdout.contains("--replay"),
        "root help should keep replay off the bare surface"
    );
    assert!(
        !stdout.contains("--scenario"),
        "root help should keep scenario off the bare surface"
    );
    assert!(
        !stdout.contains("--deterministic"),
        "root help should keep deterministic off the bare surface"
    );
    assert!(
        !stdout.contains("--exit-on-finish"),
        "root help should keep advanced tui flags off the bare surface"
    );
}
#[test]
fn tui_subcommand_help_surfaces_direct_continue_recovery_flag() {
    let output = run_harness(["tui", "--help"]);

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--continue <SESSION>"));
    assert!(stdout.contains("--replay <REPLAY>"));
}
#[test]
fn command_palette_includes_task5_session_actions() {
    let palette_commands = Action::palette_commands();
    let palette_surface = palette_commands
        .iter()
        .map(|command| {
            format!(
                "{}:{}",
                command.id,
                Action::palette_command_description(command.id)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();

    assert!(
        !palette_surface.contains("open_event_log:")
            && !palette_surface.contains("open_diff_review:")
            && palette_surface.contains("help:show shortcuts and tui controls"),
        "expected the ctrl-p surface to expose help without removed event-log or stale diff-review commands, got:
{palette_surface}"
    );
    assert!(
        palette_surface.contains("new_session:start a fresh live session")
            && palette_surface.contains("resume_session:continue a prior session when resumable"),
        "expected task-5 ctrl-p surface to include live session actions, got:\n{palette_surface}"
    );
}
