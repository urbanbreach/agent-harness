use harness_core::event::{
    ActorKind, AgentSpawnedEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, EditAppliedEvent, EditProposedEvent, EventActor,
    EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFinishedEvent, RunStartedEvent, StaleDetectedEvent, TaskCompletedEvent, TaskResultLateEvent,
    TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::app::{
    set_pending_live_launch_metadata, AppState, LaunchMetadata, SessionHistoryEntry,
};
use harness_tui::{run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::cmp;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

#[path = "support/visual_renderer.rs"]
mod visual_renderer;

use visual_renderer::{render_parser_to_image, TerminalRenderConfig};

const PTY_RENDER_CONFIG: TerminalRenderConfig = TerminalRenderConfig::new(16, 30);

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const STABLE_WINDOW: Duration = Duration::from_millis(180);
const STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const ORCHESTRATION_EVENT_DELAY: Duration = Duration::from_millis(250);
const PRESERVED_DRAFT_TEXT: &str = "keep this draft";
const STARTUP_LAUNCHER_READY_MARKER: &str = "Ask anything...";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PtyGeometry {
    cols: u16,
    rows: u16,
}

impl PtyGeometry {
    const MINIMUM_SIGNOFF: Self = Self { cols: 80, rows: 24 };
    const HALF_SCREEN_SPLIT: Self = Self { cols: 80, rows: 48 };
    const SPLIT_TIER_WINDOW: Self = Self { cols: 96, rows: 40 };
    const SIX_WINDOW_DENSE: Self = Self { cols: 60, rows: 18 };
    const PRIMARY_SIGNOFF: Self = Self {
        cols: 100,
        rows: 30,
    };
    const WIDE_SIGNOFF: Self = Self {
        cols: 160,
        rows: 30,
    };
    const SIDEBAR_SIGNOFF: Self = Self {
        cols: 160,
        rows: 48,
    };
    const WIDE_EDIT_SIGNOFF: Self = Self {
        cols: 220,
        rows: 30,
    };

    fn pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn parser(self) -> Parser {
        Parser::new(self.rows, self.cols, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptFixture {
    ready_marker: &'static str,
}

impl PromptFixture {
    const LIVE_COMPOSER: Self = Self {
        ready_marker: "Ctrl+p commands",
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DraftFixture {
    text: &'static str,
    response_marker: &'static str,
    submit_key: u8,
}

impl DraftFixture {
    const HELLO_WORLD: Self = Self {
        text: "Hello from PTY",
        response_marker: "Hello world",
        submit_key: b'\r',
    };

    fn write(self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.text.as_bytes())?;
        writer.flush()
    }

    fn submit(self, writer: &mut dyn Write) -> std::io::Result<()> {
        send_key(writer, self.submit_key)
    }

    fn write_and_submit(self, writer: &mut dyn Write) -> std::io::Result<()> {
        self.write(writer)?;
        self.submit(writer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PermissionFixture {
    marker: &'static str,
}

impl PermissionFixture {
    const TOOL_CALL: Self = Self {
        marker: "Permission required",
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LiveStateFixtures {
    prompt: PromptFixture,
    draft: DraftFixture,
    permission: PermissionFixture,
}

const LIVE_STATE_FIXTURES: LiveStateFixtures = LiveStateFixtures {
    prompt: PromptFixture::LIVE_COMPOSER,
    draft: DraftFixture::HELLO_WORLD,
    permission: PermissionFixture::TOOL_CALL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelperScenario {
    StartupShell,
    StartupPalette,
    StartupSessionHistory,
    TypeFirstStartup,
    StreamedResponse,
    ToolLifecycle,
    InlineDiffParity,
    PermissionWithDraft,
    QuestionPermission,
    DetailsDrawer,
    SidebarSessionParity,
    LiveOrchestrationLifecycle,
    LiveOrchestrationStaleLateResult,
    DegradedBootstrap,
    DisconnectedStream,
    ReplayReadOnly,
}

impl HelperScenario {
    fn env_value(self) -> &'static str {
        match self {
            Self::StartupShell => "startup_shell",
            Self::StartupPalette => "startup_palette",
            Self::StartupSessionHistory => "startup_session_history",
            Self::TypeFirstStartup => "type_first_startup",
            Self::StreamedResponse => "streamed_response",
            Self::ToolLifecycle => "tool_lifecycle",
            Self::InlineDiffParity => "inline_diff_parity",
            Self::PermissionWithDraft => "permission_with_draft",
            Self::QuestionPermission => "question_permission",
            Self::DetailsDrawer => "details_drawer",
            Self::SidebarSessionParity => "sidebar_session_parity",
            Self::LiveOrchestrationLifecycle => "live_orchestration_lifecycle",
            Self::LiveOrchestrationStaleLateResult => "live_orchestration_stale_late_result",
            Self::DegradedBootstrap => "degraded_bootstrap",
            Self::DisconnectedStream => "disconnected_stream",
            Self::ReplayReadOnly => "replay_read_only",
        }
    }

    fn helper_test_name(self) -> &'static str {
        match self {
            Self::StartupShell => "pty_helper_startup_shell",
            Self::StartupPalette => "pty_helper_startup_palette",
            Self::StartupSessionHistory => "pty_helper_startup_session_history",
            Self::TypeFirstStartup => "pty_helper_type_first_startup",
            Self::StreamedResponse => "pty_helper_streamed_response",
            Self::ToolLifecycle => "pty_helper_tool_lifecycle",
            Self::InlineDiffParity => "pty_helper_inline_diff_parity",
            Self::PermissionWithDraft => "pty_helper_permission_with_draft",
            Self::QuestionPermission => "pty_helper_question_permission",
            Self::DetailsDrawer => "pty_helper_details_drawer",
            Self::SidebarSessionParity => "pty_helper_sidebar_session_parity",
            Self::LiveOrchestrationLifecycle => "pty_helper_live_orchestration_lifecycle",
            Self::LiveOrchestrationStaleLateResult => {
                "pty_helper_live_orchestration_stale_late_result"
            }
            Self::DegradedBootstrap => "pty_helper_degraded_bootstrap",
            Self::DisconnectedStream => "pty_helper_disconnected_stream",
            Self::ReplayReadOnly => "pty_helper_replay_read_only",
        }
    }
}

#[test]
fn startup_shell_renders_bottom_composer_snapshot() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let startup = capture_type_first_startup_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("type_first_startup", &startup);

    let lines = startup.lines().collect::<Vec<_>>();
    let composer_input =
        find_line_containing(&lines, LIVE_STATE_FIXTURES.draft.text).expect("composer input row");
    let mut composer_last_row = composer_input;
    while composer_last_row + 1 < lines.len() && {
        let trimmed = lines[composer_last_row + 1].trim_start();
        trimmed.starts_with('▎') || trimmed.starts_with('┃') || trimmed.starts_with('╹')
    } {
        composer_last_row += 1;
    }
    let footer_row = find_line_containing_from(&lines, composer_input + 1, "Ctrl+p commands")
        .expect("footer legend");

    assert_eq!(
        footer_row,
        composer_last_row + 1,
        "composer should keep footer hints immediately after the compact shell\n{startup}"
    );
    assert!(
        find_line_containing_in_range(&lines, composer_input, footer_row, "Current runtime:")
            .is_none(),
        "type-first startup should no longer render the live metadata row\n{startup}"
    );
    assert!(
        find_line_containing_in_range(&lines, composer_input, footer_row, "Composer ·").is_none(),
        "legacy metadata headline row should stay removed\n{startup}"
    );
}

#[test]
fn pty_e2e_snapshots_are_stable() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let type_first_startup = capture_type_first_startup_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("type_first_startup", &type_first_startup);
    assert_visual_artifact_exists("type_first_startup", PtyGeometry::PRIMARY_SIGNOFF);

    let startup_shell = capture_startup_shell_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("startup_shell", &startup_shell);
    assert_visual_artifact_exists("startup_shell", PtyGeometry::PRIMARY_SIGNOFF);

    let startup_palette = capture_startup_palette_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("startup_palette", &startup_palette);
    assert_visual_artifact_exists("startup_palette", PtyGeometry::PRIMARY_SIGNOFF);

    let startup_slash_commands =
        capture_startup_slash_commands_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("startup_slash_commands", &startup_slash_commands);
    assert_visual_artifact_exists("startup_slash_commands", PtyGeometry::PRIMARY_SIGNOFF);

    let startup_session_history =
        capture_startup_session_history_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("startup_session_history", &startup_session_history);
    assert_visual_artifact_exists("startup_session_history", PtyGeometry::PRIMARY_SIGNOFF);

    let streamed_response = capture_streamed_response_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("streamed_response", &streamed_response);
    assert_visual_artifact_exists("streamed_response", PtyGeometry::PRIMARY_SIGNOFF);

    let wide_streamed_response = capture_streamed_response_snapshot(PtyGeometry::WIDE_SIGNOFF);
    assert_or_update_snapshot("wide_streamed_response", &wide_streamed_response);
    assert_visual_artifact_exists("streamed_response", PtyGeometry::WIDE_SIGNOFF);

    let tool_lifecycle = capture_tool_lifecycle_snapshot(PtyGeometry::WIDE_EDIT_SIGNOFF);
    assert_or_update_snapshot("tool_lifecycle", &tool_lifecycle);
    assert_visual_artifact_exists("tool_lifecycle", PtyGeometry::WIDE_EDIT_SIGNOFF);

    let inline_diff_parity = capture_helper_screen_snapshot(
        HelperScenario::InlineDiffParity,
        PtyGeometry::PRIMARY_SIGNOFF,
        "The inline diff edge cases now match the harness shell.",
    );
    assert_or_update_snapshot("inline_diff_parity", &inline_diff_parity);
    assert_visual_artifact_exists("inline_diff_parity", PtyGeometry::PRIMARY_SIGNOFF);

    let permission_with_draft =
        capture_permission_with_draft_snapshot(PtyGeometry::PRIMARY_SIGNOFF);
    assert_or_update_snapshot("permission_with_draft", &permission_with_draft);
    assert_visual_artifact_exists("permission_with_draft", PtyGeometry::PRIMARY_SIGNOFF);

    let narrow_80x24 = capture_type_first_startup_snapshot(PtyGeometry::MINIMUM_SIGNOFF);
    assert_or_update_snapshot("narrow_80x24", &narrow_80x24);
    assert_visual_artifact_exists("type_first_startup", PtyGeometry::MINIMUM_SIGNOFF);

    let split_tall_startup_shell = capture_startup_shell_snapshot(PtyGeometry::HALF_SCREEN_SPLIT);
    assert_or_update_snapshot("split_tall_startup_shell", &split_tall_startup_shell);
    assert_visual_artifact_exists("startup_shell", PtyGeometry::HALF_SCREEN_SPLIT);

    let split_tier_startup_shell = capture_startup_shell_snapshot(PtyGeometry::SPLIT_TIER_WINDOW);
    assert_or_update_snapshot("split_tier_startup_shell", &split_tier_startup_shell);
    assert_visual_artifact_exists("startup_shell", PtyGeometry::SPLIT_TIER_WINDOW);

    let quarter_tile_startup_shell = capture_startup_shell_snapshot(PtyGeometry::MINIMUM_SIGNOFF);
    assert_or_update_snapshot("quarter_tile_startup_shell", &quarter_tile_startup_shell);
    assert_visual_artifact_exists("startup_shell", PtyGeometry::MINIMUM_SIGNOFF);

    let six_window_startup_shell = capture_startup_shell_snapshot(PtyGeometry::SIX_WINDOW_DENSE);
    assert_or_update_snapshot("six_window_startup_shell", &six_window_startup_shell);
    assert_visual_artifact_exists("startup_shell", PtyGeometry::SIX_WINDOW_DENSE);

    let narrow_events_surface = capture_events_tab_snapshot(PtyGeometry::MINIMUM_SIGNOFF);
    assert_or_update_snapshot("narrow_events_surface", &narrow_events_surface);
    assert_visual_artifact_exists("events_tab", PtyGeometry::MINIMUM_SIGNOFF);

    let degraded_bootstrap = capture_helper_screen_snapshot(
        HelperScenario::DegradedBootstrap,
        PtyGeometry::MINIMUM_SIGNOFF,
        "Degraded",
    );
    assert_or_update_snapshot("degraded_bootstrap", &degraded_bootstrap);
    assert_visual_artifact_exists("degraded_bootstrap", PtyGeometry::MINIMUM_SIGNOFF);

    let disconnected_stream = capture_helper_screen_snapshot(
        HelperScenario::DisconnectedStream,
        PtyGeometry::MINIMUM_SIGNOFF,
        "Disconnected",
    );
    assert_or_update_snapshot("disconnected_stream", &disconnected_stream);
    assert_visual_artifact_exists("disconnected_stream", PtyGeometry::MINIMUM_SIGNOFF);

    assert_snapshot_secrets_clean();
}

#[test]
fn snapshot_files_exist_and_are_secret_clean() {
    let snapshot_dir = snapshot_dir();
    let expected = [
        snapshot_dir.join("type_first_startup.snap"),
        snapshot_dir.join("startup_shell.snap"),
        snapshot_dir.join("startup_palette.snap"),
        snapshot_dir.join("startup_session_history.snap"),
        snapshot_dir.join("streamed_response.snap"),
        snapshot_dir.join("wide_streamed_response.snap"),
        snapshot_dir.join("tool_lifecycle.snap"),
        snapshot_dir.join("inline_diff_parity.snap"),
        snapshot_dir.join("permission_with_draft.snap"),
        snapshot_dir.join("narrow_80x24.snap"),
        snapshot_dir.join("split_tall_startup_shell.snap"),
        snapshot_dir.join("split_tier_startup_shell.snap"),
        snapshot_dir.join("quarter_tile_startup_shell.snap"),
        snapshot_dir.join("six_window_startup_shell.snap"),
        snapshot_dir.join("narrow_events_surface.snap"),
        snapshot_dir.join("degraded_bootstrap.snap"),
        snapshot_dir.join("disconnected_stream.snap"),
    ];
    let retired = [
        snapshot_dir.join("startup.snap"),
        snapshot_dir.join("after_prompt.snap"),
        snapshot_dir.join("after_tool_call.snap"),
        snapshot_dir.join("split_tall_details_drawer.snap"),
        snapshot_dir.join("six_window_details_drawer.snap"),
    ];

    for path in expected {
        assert!(path.exists(), "missing snapshot file: {}", path.display());
    }

    for path in retired {
        assert!(
            !path.exists(),
            "retired snapshot file should be removed: {}",
            path.display()
        );
    }

    assert_snapshot_secrets_clean();
}

#[test]
fn pty_helpers_support_primary_and_minimum_geometries() {
    for (geometry, expected_cols, expected_rows) in [
        (PtyGeometry::MINIMUM_SIGNOFF, 80, 24),
        (PtyGeometry::HALF_SCREEN_SPLIT, 80, 48),
        (PtyGeometry::SPLIT_TIER_WINDOW, 96, 40),
        (PtyGeometry::SIX_WINDOW_DENSE, 60, 18),
        (PtyGeometry::PRIMARY_SIGNOFF, 100, 30),
        (PtyGeometry::WIDE_SIGNOFF, 160, 30),
    ] {
        let size = geometry.pty_size();
        assert_eq!(size.cols, expected_cols);
        assert_eq!(size.rows, expected_rows);
        assert_eq!(
            geometry.parser().screen().size(),
            (expected_rows, expected_cols)
        );
    }

    let mut draft_bytes = Vec::new();
    LIVE_STATE_FIXTURES
        .draft
        .write_and_submit(&mut draft_bytes)
        .expect("serialize reusable draft fixture");
    assert_eq!(draft_bytes, b"Hello from PTY\r");
}

#[test]
fn pty_helper_type_first_startup() {
    run_helper_if_requested(HelperScenario::TypeFirstStartup);
}

#[test]
fn pty_helper_startup_shell() {
    run_helper_if_requested(HelperScenario::StartupShell);
}

#[test]
fn pty_helper_startup_palette() {
    run_helper_if_requested(HelperScenario::StartupPalette);
}

#[test]
fn pty_helper_startup_session_history() {
    run_helper_if_requested(HelperScenario::StartupSessionHistory);
}

#[test]
fn pty_helper_streamed_response() {
    run_helper_if_requested(HelperScenario::StreamedResponse);
}

#[test]
fn pty_helper_tool_lifecycle() {
    run_helper_if_requested(HelperScenario::ToolLifecycle);
}

#[test]
fn pty_helper_inline_diff_parity() {
    run_helper_if_requested(HelperScenario::InlineDiffParity);
}

#[test]
fn pty_helper_permission_with_draft() {
    run_helper_if_requested(HelperScenario::PermissionWithDraft);
}

#[test]
fn pty_helper_question_permission() {
    run_helper_if_requested(HelperScenario::QuestionPermission);
}

#[test]
fn pty_helper_details_drawer() {
    run_helper_if_requested(HelperScenario::DetailsDrawer);
}

#[test]
fn pty_helper_sidebar_session_parity() {
    run_helper_if_requested(HelperScenario::SidebarSessionParity);
}

#[test]
fn pty_helper_live_orchestration_lifecycle() {
    run_helper_if_requested(HelperScenario::LiveOrchestrationLifecycle);
}

#[test]
fn pty_helper_live_orchestration_stale_late_result() {
    run_helper_if_requested(HelperScenario::LiveOrchestrationStaleLateResult);
}

#[test]
fn pty_helper_degraded_bootstrap() {
    run_helper_if_requested(HelperScenario::DegradedBootstrap);
}

#[test]
fn pty_helper_disconnected_stream() {
    run_helper_if_requested(HelperScenario::DisconnectedStream);
}

#[test]
fn pty_helper_replay_read_only() {
    run_helper_if_requested(HelperScenario::ReplayReadOnly);
}

#[test]
fn pty_lineage_fork_selector() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| intents.lock().expect("lock lineage intents").push(intent))
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_fixture")), false, Some(sink));
    for event in fork_selector_events() {
        app.ingest_event(event);
    }

    app.execute_slash_command("fork", Some(String::new()));

    assert!(app.fork_selector_visible);
    assert!(intents.lock().expect("lock lineage intents").is_empty());
    assert_eq!(app.fork_selector_view_model().selected_cutoff_seq, Some(5));
    assert_eq!(
        app.fork_selector_view_model().rows[0].prompt_text,
        "Full session"
    );
    assert_eq!(
        app.fork_selector_view_model().rows[1].prompt_text,
        "Fork this prompt"
    );

    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Down,
        crossterm::event::KeyModifiers::NONE,
    ));

    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let intents = intents.lock().expect("lock lineage intents");
    assert_eq!(intents.len(), 1);
    match &intents[0] {
        UiIntent::ForkSession {
            stable_prefix,
            prompt_text,
            ..
        } => {
            assert_eq!(stable_prefix.cutoff_seq, 2);
            assert_eq!(prompt_text, "Fork this prompt");
        }
        intent => panic!("expected fork lineage intent, got {intent:?}"),
    }
}

fn fork_selector_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            Some("run_fixture"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "fork-fixture".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("run_fixture"),
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_fork".to_string(),
                parent_agent_id: None,
                profile: "worker".to_string(),
            }),
        ),
        envelope(
            3,
            Some("req_fork"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_fork".to_string(),
                text: "Fork this prompt".to_string(),
            }),
        ),
        envelope(
            4,
            Some("req_fork"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_fork".to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Fork this prompt".to_string(),
                request_digest: "digest-req-fork".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            5,
            Some("req_fork"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_fork".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-fork-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

#[test]
fn pty_lineage_clone_blocked_in_replay() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay_run"), replay_read_only_events());

    app.execute_slash_command("clone", Some("preserved replay draft".to_string()));

    assert_eq!(app.prompt_buffer, "preserved replay draft");
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session clone blocked: replay mode is read-only")
    );
}

#[test]
fn pty_live_details_drawer_remains_reachable() {
    if !cfg!(target_os = "linux") {
        return;
    }

    for geometry in [
        PtyGeometry::PRIMARY_SIGNOFF,
        PtyGeometry::HALF_SCREEN_SPLIT,
        PtyGeometry::SPLIT_TIER_WINDOW,
        PtyGeometry::SIX_WINDOW_DENSE,
    ] {
        let mut helper = spawn_helper_pty(HelperScenario::DetailsDrawer, geometry);
        wait_for_screen_contains(
            &mut helper.parser,
            &helper.output_rx,
            LIVE_STATE_FIXTURES.prompt.ready_marker,
            STARTUP_TIMEOUT,
        )
        .expect("wait for startup before operator sidebar flow");

        send_ctrl_tab(helper.writer.as_mut())
            .expect("focus transcript before opening operator sidebar");
        send_key(helper.writer.as_mut(), b'i').expect("open operator sidebar");

        if geometry == PtyGeometry::SIX_WINDOW_DENSE {
            let screen = wait_for_screen_contains(
                &mut helper.parser,
                &helper.output_rx,
                "Ctrl+p commands",
                MARKER_TIMEOUT,
            )
            .expect("wait for dense session shell after sidebar toggle");

            assert!(!screen.contains("Context"));
            assert!(screen.contains("Ctrl+p commands"));
        } else {
            let screen = wait_for_screen_contains(
                &mut helper.parser,
                &helper.output_rx,
                "▼ MCP",
                MARKER_TIMEOUT,
            )
            .expect("wait for operator sidebar markers");

            assert!(screen.contains("▼ MCP"));
        }

        terminate_child(helper.child);
    }
}

#[test]
fn operator_sidebar_matches_information_architecture() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let mut helper = spawn_helper_pty(
        HelperScenario::SidebarSessionParity,
        PtyGeometry::SIDEBAR_SIGNOFF,
    );
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Inspect sidebar parity",
        STARTUP_TIMEOUT,
    )
    .expect("wait for sidebar parity startup render");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "src/ui_secondary.rs",
        MARKER_TIMEOUT,
    )
    .expect("wait for sidebar parity markers");

    assert_markers_in_order(
        &screen,
        &[
            "Inspect sidebar parity",
            "▼ Subagents",
            "▼ MCP",
            "▼ LSP",
            "▼ Modified Files",
        ],
    );
    assert!(!screen.contains("Current runtime:"));
    assert_screen_contains_all(
        &screen,
        &[
            "• websearch Connected",
            "• plan",
            "• reporter ✓ Wakeup report visible",
            "• rust",
            "src/ui_secondary.rs",
            "src/layout.rs",
            "src/theme.rs",
            "[BACKGROUND TASK COMPLETED]",
            "background_output(request_id=\"req_plan\")",
            "task(session_id=\"task_plan\")",
        ],
    );
    assert!(!screen.contains("Todo ·"));
    assert!(!screen.contains("Recovery ·"));
    assert!(!screen.contains("Network ·"));
    assert!(!screen.contains("Batch ·"));

    terminate_child(helper.child);
}

#[test]
fn question_permission_renders_inline_prompt_in_pty() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let mut helper = spawn_helper_pty(
        HelperScenario::QuestionPermission,
        PtyGeometry::PRIMARY_SIGNOFF,
    );
    wait_for_live_startup(&mut helper);

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Type your own answer",
        MARKER_TIMEOUT,
    )
    .expect("wait for question permission marker");

    assert_screen_contains_all(
        &screen,
        &[
            "Pick one",
            "Type your own answer",
            "↑↓ select",
            "enter submit",
            "esc dismiss",
        ],
    );
    write_visual_artifact(
        "question_permission",
        PtyGeometry::PRIMARY_SIGNOFF,
        &helper.parser,
    );
    terminate_child(helper.child);
}

#[test]
fn pty_live_orchestration_drawer_and_status() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let mut helper = spawn_helper_pty(
        HelperScenario::LiveOrchestrationLifecycle,
        PtyGeometry::WIDE_SIGNOFF,
    );
    wait_for_live_startup(&mut helper);
    show_live_operator_sidebar(&mut helper);

    let queued_screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "▼ MCP",
        MARKER_TIMEOUT,
    )
    .expect("wait for operator sidebar sections");
    assert_screen_contains_all(&queued_screen, &["▼ MCP", "▶ Modified Files"]);
    assert!(!queued_screen.contains("No modified files"));

    thread::sleep(ORCHESTRATION_EVENT_DELAY + STABLE_WINDOW);
    drain_output(&mut helper.parser, &helper.output_rx);
    let completed_initial_screen = helper.parser.screen().contents();
    let completed_screen = stabilize_screen(
        &mut helper.parser,
        &helper.output_rx,
        completed_initial_screen,
    );
    assert_screen_contains_all(
        &completed_screen,
        &["▼ MCP", "▶ Modified Files", "Ctrl+p commands"],
    );
    assert!(!completed_screen.contains("No modified files"));
    assert!(!completed_screen.contains("Todo ·"));
    assert!(!completed_screen.contains("MCP · idle"));
    assert!(!completed_screen.contains("Network · 1"));
    assert!(!completed_screen.contains("Batch · 1"));
    assert!(!completed_screen.contains("LSP · idle"));
    assert!(!completed_screen.contains("LSP · 1"));
    assert!(!completed_screen.contains("orch 0a 0q 0r 0s"));

    terminate_child(helper.child);
}

#[test]
fn pty_live_orchestration_stale_late_result_flow() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let mut helper = spawn_helper_pty(
        HelperScenario::LiveOrchestrationStaleLateResult,
        PtyGeometry::WIDE_SIGNOFF,
    );
    wait_for_live_startup(&mut helper);
    show_live_operator_sidebar(&mut helper);

    let late_result_screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "▼ MCP",
        MARKER_TIMEOUT,
    )
    .expect("wait for late result sidebar state");
    assert_screen_contains_all(&late_result_screen, &["▼ MCP", "▶ Modified Files"]);
    assert!(!late_result_screen.contains("No modified files"));

    terminate_child(helper.child);
}

#[test]
fn replay_mode_never_emits_submit_prompt_intent() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let marker_dir = tempfile::tempdir().expect("create replay marker tempdir");
    let marker_path = marker_dir.path().join("replay-intent.txt");

    let mut helper = spawn_helper_pty_with_intent_marker(
        HelperScenario::ReplayReadOnly,
        PtyGeometry::PRIMARY_SIGNOFF,
        &marker_path,
    );
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "q quit",
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay startup render");

    send_ctrl_tab(helper.writer.as_mut()).expect("focus replay details surface");
    send_ctrl_tab(helper.writer.as_mut()).expect("attempt replay prompt focus");
    helper
        .writer
        .write_all(b"blocked in replay")
        .expect("type replay draft");
    helper.writer.flush().expect("flush replay draft");
    send_key(helper.writer.as_mut(), b'\r').expect("attempt replay submit");

    let current = helper.parser.screen().contents();
    let stable = stabilize_screen(&mut helper.parser, &helper.output_rx, current);

    assert!(
        stable.contains("q quit"),
        "expected replay status to stay visible after submit attempt\n{stable}"
    );
    assert!(
        stable.contains("Replay · read-only"),
        "expected replay composer to render a read-only title\n{stable}"
    );
    assert!(
        stable.contains("Replay is read-only"),
        "expected replay composer body to explain read-only behavior\n{stable}"
    );
    assert!(
        !stable.contains("Enter send"),
        "replay surface unexpectedly exposed a send-capable dock\n{stable}"
    );
    assert!(
        !stable.contains("blocked in replay"),
        "replay surface unexpectedly preserved typed draft text\n{stable}"
    );
    assert!(
        !marker_path.exists(),
        "replay submit unexpectedly emitted a UI intent"
    );

    terminate_child(helper.child);
}

#[test]
fn startup_shell_displays_meaningful_mock_launch_metadata() {
    if !cfg!(target_os = "linux") {
        return;
    }

    for geometry in [
        PtyGeometry::PRIMARY_SIGNOFF,
        PtyGeometry::HALF_SCREEN_SPLIT,
        PtyGeometry::SPLIT_TIER_WINDOW,
        PtyGeometry::MINIMUM_SIGNOFF,
        PtyGeometry::SIX_WINDOW_DENSE,
    ] {
        let startup_shell = capture_startup_shell_snapshot(geometry);
        assert!(!startup_shell.contains("Launch: worker · model-1"));
        assert!(startup_shell.contains("Ask anything..."));
        assert!(startup_shell.contains("Worker model-1 mock"));
        assert!(!startup_shell.contains("Dispatch a new run"));
        assert!(!startup_shell.contains("Actions:"));
        assert!(!startup_shell.contains("provider unknown"));
        assert!(startup_shell.contains("╻ ╻  ┏━┓  ┏━┓  ┏┓╻") || startup_shell.contains("Harness"));
    }
}

fn run_helper_if_requested(scenario: HelperScenario) {
    if !cfg!(target_os = "linux") {
        return;
    }
    if helper_scenario_from_env() != Some(scenario) {
        return;
    }

    let run_dir = tempfile::tempdir().expect("create temp helper run dir");
    let (tx, rx) = mpsc::channel::<LiveUpdate>();

    let on_ui_intent = match scenario {
        HelperScenario::StreamedResponse => Some(streamed_response_intent_handler(tx.clone())),
        _ => None,
    };

    match scenario {
        HelperScenario::StartupShell
        | HelperScenario::StartupPalette
        | HelperScenario::StartupSessionHistory => {
            set_pending_live_launch_metadata(helper_startup_launch_metadata());
            run_tui_with_options(TuiOptions {
                mode: TuiMode::Startup {
                    session_history_entries: startup_session_history_entries(),
                },
                exit_on_finish: false,
                on_ui_intent,
                keybindings: None,
                toggles: None,
                preserve_terminal_on_exit: false,
                slash_command_templates: None,
            })
            .expect("run startup helper tui");
            return;
        }
        HelperScenario::TypeFirstStartup | HelperScenario::StreamedResponse => {}
        HelperScenario::ToolLifecycle => {
            write_tool_lifecycle_diff_fixture(run_dir.path());
            let tool_tx = tx.clone();
            thread::spawn(move || {
                for event in tool_lifecycle_events() {
                    tool_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .expect("send tool lifecycle events");
                }
                thread::park();
            });
        }
        HelperScenario::InlineDiffParity => {
            write_inline_diff_parity_fixtures(run_dir.path());
            let parity_tx = tx.clone();
            thread::spawn(move || {
                for event in inline_diff_parity_events() {
                    parity_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .expect("send inline diff parity events");
                }
                thread::park();
            });
        }
        HelperScenario::PermissionWithDraft => {
            let permission_tx = tx.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(250));
                permission_tx
                    .send(LiveUpdate::Event(Box::new(permission_requested_event(
                        1,
                        "perm_pty",
                        "tool_call_pty",
                    ))))
                    .expect("send permission request event");
                thread::park();
            });
        }
        HelperScenario::QuestionPermission => {
            let question_tx = tx.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(250));
                question_tx
                    .send(LiveUpdate::Event(Box::new(
                        question_permission_requested_event(
                            1,
                            "perm_question_pty",
                            "tool_call_question_pty",
                        ),
                    )))
                    .expect("send question permission request event");
                thread::park();
            });
        }
        HelperScenario::DetailsDrawer => {
            let details_tx = tx.clone();
            thread::spawn(move || {
                for event in details_drawer_events() {
                    details_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .expect("send operator sidebar events");
                }
                thread::park();
            });
        }
        HelperScenario::SidebarSessionParity => {
            let sidebar_tx = tx.clone();
            thread::spawn(move || {
                for event in sidebar_session_parity_events() {
                    sidebar_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .expect("send sidebar parity events");
                }
                thread::park();
            });
        }
        HelperScenario::LiveOrchestrationLifecycle => {
            send_live_updates(&tx, orchestration_base_events());
            spawn_phased_live_updates(
                tx.clone(),
                vec![
                    (
                        ORCHESTRATION_EVENT_DELAY,
                        orchestration_lifecycle_queued_events(),
                    ),
                    (
                        ORCHESTRATION_EVENT_DELAY,
                        orchestration_lifecycle_started_events(),
                    ),
                    (
                        ORCHESTRATION_EVENT_DELAY,
                        orchestration_lifecycle_completed_events(),
                    ),
                ],
            );
        }
        HelperScenario::LiveOrchestrationStaleLateResult => {
            send_live_updates(&tx, orchestration_base_events());
            spawn_phased_live_updates(
                tx.clone(),
                vec![
                    (
                        ORCHESTRATION_EVENT_DELAY,
                        orchestration_stale_started_events(),
                    ),
                    (
                        ORCHESTRATION_EVENT_DELAY,
                        orchestration_stale_detected_events(),
                    ),
                    (
                        ORCHESTRATION_EVENT_DELAY,
                        orchestration_late_result_events(),
                    ),
                ],
            );
        }
        HelperScenario::DegradedBootstrap => {
            tx.send(LiveUpdate::Status(
                "live stream lagged by 2; replaying from seq 1".to_string(),
            ))
            .expect("send degraded bootstrap status");
        }
        HelperScenario::DisconnectedStream => {
            drop(tx);
            run_tui_with_options(TuiOptions {
                mode: TuiMode::Live {
                    run_dir: run_dir.path().to_path_buf(),
                    historical_events: Vec::new(),
                    session_history_entries: Vec::new(),
                    update_rx: rx,
                    compact_session_supported: false,
                },
                exit_on_finish: false,
                on_ui_intent,
                keybindings: None,
                toggles: None,
                preserve_terminal_on_exit: false,
                slash_command_templates: None,
            })
            .expect("run disconnected helper tui");
            return;
        }
        HelperScenario::ReplayReadOnly => {
            let marker_path = replay_intent_marker_path_from_env();
            let replay_events = replay_read_only_events();
            run_tui_with_options(TuiOptions {
                mode: TuiMode::Replay {
                    run_dir: run_dir.path().to_path_buf(),
                    events: replay_events,
                },
                exit_on_finish: false,
                on_ui_intent: marker_path.map(replay_intent_handler),
                keybindings: None,
                toggles: None,
                preserve_terminal_on_exit: false,
                slash_command_templates: None,
            })
            .expect("run replay read-only helper tui");
            return;
        }
    }

    let _keepalive = tx;
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            update_rx: rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        slash_command_templates: None,
    })
    .expect("run helper tui");
}

fn replay_intent_handler(marker_path: PathBuf) -> Arc<dyn Fn(UiIntent) + Send + Sync> {
    Arc::new(move |intent: UiIntent| {
        fs::write(&marker_path, format!("{intent:?}"))
            .expect("persist unexpected replay intent marker");
    })
}

fn helper_startup_launch_metadata() -> LaunchMetadata {
    LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo")
}

fn replay_read_only_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            Some("run_fixture"),
            EventV1::RunStarted(RunStartedEvent {
                run_name: "replay-fixture".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            2,
            Some("run_fixture"),
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn streamed_response_intent_handler(tx: Sender<LiveUpdate>) -> Arc<dyn Fn(UiIntent) + Send + Sync> {
    let submitted = Arc::new(Mutex::new(false));
    Arc::new(move |intent: UiIntent| {
        let UiIntent::SubmitPrompt { text, .. } = intent else {
            return;
        };

        let mut submitted = submitted.lock().expect("lock submit guard");
        if *submitted {
            return;
        }
        *submitted = true;
        drop(submitted);

        let tx = tx.clone();
        thread::spawn(move || {
            for event in streamed_response_events(&text) {
                tx.send(LiveUpdate::Event(Box::new(event)))
                    .expect("send helper live event");
                thread::sleep(Duration::from_millis(40));
            }
        });
    })
}

fn streamed_response_events(text: &str) -> Vec<EventEnvelopeV1> {
    let request_id = "req_pty_001";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: text.to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: text.to_string(),
                request_digest: "digest-req-pty-001".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Hello".to_string(),
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: " world".to_string(),
            }),
        ),
        envelope(
            5,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output-pty-001".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn tool_lifecycle_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_tool_lifecycle";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect tool activity".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Inspect tool activity".to_string(),
                request_digest: "digest-tool-lifecycle-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: r#"{"path":"src/ui.rs","start_line":1,"limit":24}"#.to_string(),
                args_digest: "digest-tool-lifecycle-read-args".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("fs.read".to_string()),
                    alias_source_tool_id: Some("read".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_read".to_string(),
            }),
        ),
        envelope(
            5,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_read".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read from src/ui.rs".to_string()),
                output_digest: Some("digest-tool-lifecycle-read-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("fs.read".to_string()),
                    alias_source_tool_id: Some("read".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            6,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_edit".to_string(),
                tool_id: "edit.hashline_apply".to_string(),
                args_summary: r#"{"path":"crates/harness-tui/src/ui.rs"}"#.to_string(),
                args_digest: "digest-tool-lifecycle-edit-args".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            7,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_edit".to_string(),
            }),
        ),
        envelope(
            8,
            Some("tc_edit"),
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit_tool_lifecycle".to_string(),
                path: "crates/harness-tui/src/ui.rs".to_string(),
                summary: "Remove diff review surface".to_string(),
                patch_digest: "digest-tool-lifecycle-edit-patch".to_string(),
            }),
        ),
        envelope(
            9,
            Some("tc_edit"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_tool_lifecycle".to_string(),
                path: "crates/harness-tui/src/ui.rs".to_string(),
                new_file_digest: "digest-tool-lifecycle-edit-file".to_string(),
                diff_rel_path: Some("artifacts/tool-lifecycle-inline.diff".to_string()),
                diff_digest: Some("digest-tool-lifecycle-edit-diff".to_string()),
            }),
        ),
        envelope(
            10,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_edit".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Patched crates/harness-tui/src/ui.rs".to_string()),
                output_digest: Some("digest-tool-lifecycle-edit-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            11,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_task".to_string(),
                tool_id: "task".to_string(),
                args_summary:
                    r#"{"description":"audit tool lifecycle parity","subagent_type":"researcher"}"#
                        .to_string(),
                args_digest: "digest-tool-lifecycle-task-args".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("agent.spawn".to_string()),
                    alias_source_tool_id: Some("task".to_string()),
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some(request_id.to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            12,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_task".to_string(),
            }),
        ),
        envelope(
            13,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_task".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Found the whole-tool parity path.".to_string()),
                output_digest: Some("digest-tool-lifecycle-task-output".to_string()),
                output_json: Some(serde_json::json!({
                    "description": "audit tool lifecycle parity",
                    "profile": "researcher",
                    "mode": "foreground",
                    "status": "completed",
                    "result_summary": "Found the whole-tool parity path.",
                    "child_tool_call_count": 2,
                    "child_session_id": "agent_worker",
                    "child_request_id": "req_child"
                })),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("agent.spawn".to_string()),
                    alias_source_tool_id: Some("task".to_string()),
                    lineage: Some(harness_core::event::TaskLineageMetadata {
                        parent_tool_call_id: Some("tc_task".to_string()),
                        parent_request_id: Some(request_id.to_string()),
                        child_session_id: Some("agent_worker".to_string()),
                        child_request_id: Some("req_child".to_string()),
                        ..harness_core::event::TaskLineageMetadata::default()
                    }),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            14,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_shell".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"cargo test -p harness-tui","cwd":"/workspace"}"#
                    .to_string(),
                args_digest: "digest-tool-lifecycle-shell-args".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            15,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell".to_string(),
            }),
        ),
        envelope(
            16,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_shell".to_string(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: snapshot mismatch".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            17,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Tool summaries are now easier to scan, and edits stay inline.".to_string(),
            }),
        ),
        envelope(
            18,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-tool-lifecycle-response".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn inline_diff_parity_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_inline_diff_parity";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Make inline diffs match the harness shell".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "model-1".to_string(),
                prompt_summary: "Make inline diffs match the harness shell".to_string(),
                request_digest: "digest-inline-diff-parity-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "I verified the current transcript behavior before patching it.".to_string(),
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_inline_apply".to_string(),
                tool_id: "apply_patch".to_string(),
                args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
                args_digest: "digest-inline-diff-parity-apply-args".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("apply_patch".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            5,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_inline_apply".to_string(),
            }),
        ),
        envelope(
            6,
            Some("tc_inline_apply"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_inline_diff_rename".to_string(),
                path: "src/session_diff.rs".to_string(),
                new_file_digest: "digest-inline-diff-rename-file".to_string(),
                diff_rel_path: Some("artifacts/inline-diff-rename.diff".to_string()),
                diff_digest: Some("digest-inline-diff-rename".to_string()),
            }),
        ),
        envelope(
            7,
            Some("tc_inline_apply"),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_inline_diff_wrap".to_string(),
                path: "docs/transcript.md".to_string(),
                new_file_digest: "digest-inline-diff-wrap-file".to_string(),
                diff_rel_path: Some("artifacts/inline-diff-wrap.diff".to_string()),
                diff_digest: Some("digest-inline-diff-wrap".to_string()),
            }),
        ),
        envelope(
            8,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_inline_apply".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Success. Updated the following files".to_string()),
                output_digest: Some("digest-inline-diff-parity-apply-output".to_string()),
                output_json: Some(serde_json::json!({
                    "files": ["M src/session_diff.rs", "M docs/transcript.md"],
                    "edits": [
                        {
                            "edit_id": "edit_inline_diff_rename",
                            "path": "src/session_diff.rs",
                            "summary": "apply patch move src/session_turn.rs -> src/session_diff.rs",
                            "deleted": false,
                            "diff_rel_path": "artifacts/inline-diff-rename.diff",
                            "diff_digest": "digest-inline-diff-rename"
                        },
                        {
                            "edit_id": "edit_inline_diff_wrap",
                            "path": "docs/transcript.md",
                            "summary": "apply patch update docs/transcript.md",
                            "deleted": false,
                            "diff_rel_path": "artifacts/inline-diff-wrap.diff",
                            "diff_digest": "digest-inline-diff-wrap"
                        }
                    ]
                })),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("apply_patch".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            9,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read_after_inline_apply".to_string(),
                tool_id: "read".to_string(),
                args_summary: r#"{"filePath":"docs/transcript.md","offset":1,"limit":40}"#
                    .to_string(),
                args_digest: "digest-inline-diff-parity-read-args".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("fs.read".to_string()),
                    alias_source_tool_id: Some("read".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            10,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_read_after_inline_apply".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("40 lines read from docs/transcript.md".to_string()),
                output_digest: Some("digest-inline-diff-parity-read-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("fs.read".to_string()),
                    alias_source_tool_id: Some("read".to_string()),
                    ..harness_core::event::ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            11,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "The inline diff edge cases now match the harness shell.".to_string(),
            }),
        ),
        envelope(
            12,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-inline-diff-parity-response".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn write_tool_lifecycle_diff_fixture(run_dir: &Path) {
    let artifacts_dir = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("create tool lifecycle artifacts dir");
    fs::write(
        artifacts_dir.join("tool-lifecycle-inline.diff"),
        "--- crates/harness-tui/src/ui.rs\n+++ crates/harness-tui/src/ui.rs\n@@ -44,8 +44,7 @@\n use ui_secondary::{\n-    render_diff_tab, render_events_tab, render_help_tab,\n+    render_events_tab, render_help_tab, render_live_details_overlay,\n     render_operator_sidebar,\n };\n",
    )
    .expect("write lifecycle diff fixture");
}

fn write_inline_diff_parity_fixtures(run_dir: &Path) {
    let artifacts_dir = run_dir.join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("create inline diff parity artifacts dir");
    fs::write(
        artifacts_dir.join("inline-diff-rename.diff"),
        "--- src/session_turn.rs\n+++ src/session_diff.rs\n@@ -1,1 +1,1 @@\n-pub fn render_session_turn_diff() {}\n+pub fn render_session_diff_view() {}\n",
    )
    .expect("write inline diff parity rename fixture");
    fs::write(
        artifacts_dir.join("inline-diff-wrap.diff"),
        "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n",
    )
    .expect("write inline diff parity wrap fixture");
}

fn details_drawer_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_details_drawer";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect the operator sidebar".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Inspect the operator sidebar".to_string(),
                request_digest: "digest-details-drawer".to_string(),
                metadata: None,
            }),
        ),
    ]
}

fn sidebar_session_parity_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_sidebar_parity";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect sidebar parity".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect sidebar parity".to_string(),
                request_digest: "digest-sidebar-parity".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            None,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "w1".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            4,
            Some(request_id),
            EventActor::new(ActorKind::System, None),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_plan".to_string(),
                state: TaskScheduleState::Queued,
                queue_key: Some("agent:queue:plan".to_string()),
            }),
        ),
        envelope_with_actor(
            5,
            Some(request_id),
            EventActor::new(ActorKind::Worker, Some("w1".to_string())),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_review".to_string(),
                state: TaskScheduleState::Queued,
                queue_key: Some("tool:fs.read".to_string()),
            }),
        ),
        envelope_with_actor(
            6,
            Some(request_id),
            EventActor::new(ActorKind::Worker, Some("w1".to_string())),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_follow_up".to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("agent:queue:follow-up".to_string()),
            }),
        ),
        envelope(
            7,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_call_search".to_string(),
                tool_id: "search.web".to_string(),
                args_summary: serde_json::json!({"query": "harness sidebar"}).to_string(),
                args_digest: "digest-search-web".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        envelope(
            8,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_search".to_string(),
            }),
        ),
        envelope(
            9,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_call_search".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Fetched harness sidebar examples".to_string()),
                output_digest: Some("digest-search-web-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("search.web".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        envelope(
            10,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_call_batch".to_string(),
                tool_id: "tool.batch".to_string(),
                args_summary: serde_json::json!({"requested_call_count": 2}).to_string(),
                args_digest: "digest-tool-batch".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("tool.batch".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        envelope(
            11,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_batch".to_string(),
            }),
        ),
        envelope(
            12,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool_call_lsp".to_string(),
                tool_id: "code.lsp".to_string(),
                args_summary: serde_json::json!({
                    "operation": "goto_definition",
                    "path": "src/ui_secondary.rs"
                })
                .to_string(),
                args_digest: "digest-tool-lsp".to_string(),
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        envelope(
            13,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool_call_lsp".to_string(),
            }),
        ),
        envelope(
            14,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool_call_lsp".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Found definition in src/ui_secondary.rs".to_string()),
                output_digest: Some("digest-tool-lsp-output".to_string()),
                output_json: None,
                metadata: Some(harness_core::event::ToolCallMetadata {
                    canonical_tool_id: Some("code.lsp".to_string()),
                    ..Default::default()
                }),
            }),
        ),
        envelope(
            15,
            Some(request_id),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_sidebar_1".to_string(),
                path: "src/ui_secondary.rs".to_string(),
                new_file_digest: "digest-edit-sidebar-1".to_string(),
                diff_rel_path: None,
                diff_digest: None,
            }),
        ),
        envelope(
            16,
            Some(request_id),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_sidebar_2".to_string(),
                path: "src/layout.rs".to_string(),
                new_file_digest: "digest-edit-sidebar-2".to_string(),
                diff_rel_path: None,
                diff_digest: None,
            }),
        ),
        envelope(
            17,
            Some(request_id),
            EventV1::EditApplied(EditAppliedEvent {
                edit_id: "edit_sidebar_3".to_string(),
                path: "src/theme.rs".to_string(),
                new_file_digest: "digest-edit-sidebar-3".to_string(),
                diff_rel_path: None,
                diff_digest: None,
            }),
        ),
        envelope(
            18,
            Some("background_task_notification:req_plan"),
            EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "task_plan".to_string(),
                child_request_id: "req_plan".to_string(),
                task_id: "task_plan".to_string(),
                description: "Inspect sidebar parity".to_string(),
                status: BackgroundTaskNotificationStatus::Completed,
                summary: "Plan subagent finished".to_string(),
                terminal_event_id: "evt_plan_done".to_string(),
                terminal_task_id: "task_plan".to_string(),
                delivered_turn_request_id: Some("req_sidebar_wakeup".to_string()),
            }),
        ),
        envelope(
            19,
            None,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_reporter".to_string(),
                profile: "reporter".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
            }),
        ),
        envelope(
            20,
            Some("background_task_notification:req_reporter"),
            EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
                parent_session_id: "run_fixture".to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                child_session_id: "agent_reporter".to_string(),
                child_request_id: "req_reporter".to_string(),
                task_id: "task_reporter".to_string(),
                description: "Show sidebar wakeup".to_string(),
                status: BackgroundTaskNotificationStatus::Completed,
                summary: "Wakeup report visible".to_string(),
                terminal_event_id: "evt_reporter_done".to_string(),
                terminal_task_id: "task_reporter".to_string(),
                delivered_turn_request_id: Some("req_reporter_wakeup".to_string()),
            }),
        ),
    ]
}

fn orchestration_base_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_orch";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.to_string(),
                text: "Inspect live orchestration".to_string(),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.to_string(),
                provider_id: "mock".to_string(),
                model_id: "m1".to_string(),
                prompt_summary: "Inspect live orchestration".to_string(),
                request_digest: "digest-orch-request".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Done.".to_string(),
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-orch-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ]
}

fn orchestration_lifecycle_queued_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            5,
            None,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "w1".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            6,
            Some("req_orch"),
            EventActor::new(ActorKind::Worker, Some("w1".to_string())),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_live_cycle".to_string(),
                state: TaskScheduleState::Queued,
                queue_key: Some("agent:queue:deep".to_string()),
            }),
        ),
    ]
}

fn orchestration_lifecycle_started_events() -> Vec<EventEnvelopeV1> {
    vec![envelope_with_actor(
        7,
        Some("req_orch"),
        EventActor::new(ActorKind::Worker, Some("w1".to_string())),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_live_cycle".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:queue:deep".to_string()),
        }),
    )]
}

fn orchestration_lifecycle_completed_events() -> Vec<EventEnvelopeV1> {
    vec![envelope_with_actor(
        8,
        Some("req_orch"),
        EventActor::new(ActorKind::Worker, Some("w1".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_live_cycle".to_string(),
            result_summary: "done".to_string(),
            result_digest: "digest-live-cycle".to_string(),
            metadata: None,
        }),
    )]
}

fn orchestration_stale_started_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            5,
            None,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "w1".to_string(),
                profile: "deep".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            6,
            Some("req_orch"),
            EventActor::new(ActorKind::Worker, Some("w1".to_string())),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_stale".to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("agent:running:stale".to_string()),
            }),
        ),
    ]
}

fn orchestration_stale_detected_events() -> Vec<EventEnvelopeV1> {
    vec![envelope_with_actor(
        7,
        Some("req_orch"),
        EventActor::new(ActorKind::Worker, Some("w1".to_string())),
        EventV1::StaleDetected(StaleDetectedEvent {
            task_id: "task_stale".to_string(),
            stale_for_ms: 3001,
        }),
    )]
}

fn orchestration_late_result_events() -> Vec<EventEnvelopeV1> {
    vec![envelope_with_actor(
        8,
        Some("req_orch"),
        EventActor::new(ActorKind::Worker, Some("w1".to_string())),
        EventV1::TaskResultLate(TaskResultLateEvent {
            task_id: "task_stale".to_string(),
            result_digest: "digest-late-result".to_string(),
        }),
    )]
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.to_string()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: "digest-perm-pty".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    )
}

fn question_permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.to_string()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Pick one",
                    "header": "Choice",
                    "options": [{"label": "A", "description": "Option A"}],
                    "multiple": false,
                }]
            })
            .to_string(),
            request_digest: "digest-question-perm-pty".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    )
}

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("pty-helper".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

fn envelope_with_actor(
    seq: u64,
    correlation_id: Option<&str>,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

fn capture_type_first_startup_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::TypeFirstStartup, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for helper startup render");

    LIVE_STATE_FIXTURES
        .draft
        .write(helper.writer.as_mut())
        .expect("type startup draft");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.draft.text,
        MARKER_TIMEOUT,
    )
    .expect("wait for typed startup draft");

    write_visual_artifact("type_first_startup", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_startup_shell_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::StartupShell, geometry);
    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher shell render");

    write_visual_artifact("startup_shell", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_startup_palette_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::StartupPalette, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher before opening palette");

    send_key(helper.writer.as_mut(), 0x10).expect("open startup command palette");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Commands",
        MARKER_TIMEOUT,
    )
    .expect("wait for startup command palette");

    write_visual_artifact("startup_palette", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_startup_slash_commands_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::StartupShell, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher before opening slash commands");

    helper
        .writer
        .write_all(b"/")
        .expect("type startup slash command trigger");
    helper
        .writer
        .flush()
        .expect("flush startup slash command trigger");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "/new",
        MARKER_TIMEOUT,
    )
    .expect("wait for startup slash command overlay");

    write_visual_artifact("startup_slash_commands", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_startup_session_history_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::StartupSessionHistory, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher before opening history");

    send_key(helper.writer.as_mut(), 0x10).expect("open startup command palette");
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Commands",
        MARKER_TIMEOUT,
    )
    .expect("wait for startup palette before opening history");
    helper
        .writer
        .write_all(b"resume")
        .expect("filter startup command palette to resume");
    helper
        .writer
        .flush()
        .expect("flush startup command palette filter");
    send_key(helper.writer.as_mut(), b'\r').expect("enter startup session history");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Continue session",
        MARKER_TIMEOUT,
    )
    .expect("wait for startup session history picker");

    write_visual_artifact("startup_session_history", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_streamed_response_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::StreamedResponse, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup before prompt submit");

    LIVE_STATE_FIXTURES
        .draft
        .write_and_submit(helper.writer.as_mut())
        .expect("submit streamed response draft");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.draft.response_marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for streamed response marker");
    let screen = stabilize_screen(&mut helper.parser, &helper.output_rx, screen);

    write_visual_artifact("streamed_response", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_tool_lifecycle_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::ToolLifecycle, geometry);
    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Tool summaries are now easier to scan, and edits stay inline.",
        MARKER_TIMEOUT,
    )
    .expect("wait for tool lifecycle marker");

    write_visual_artifact("tool_lifecycle", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_permission_with_draft_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::PermissionWithDraft, geometry);
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup before permission overlay");

    helper
        .writer
        .write_all(PRESERVED_DRAFT_TEXT.as_bytes())
        .expect("type preserved draft");
    helper.writer.flush().expect("flush preserved draft");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.permission.marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for permission overlay marker");

    assert!(
        screen.contains("Allow always"),
        "permission snapshot lost reference dialog controls"
    );
    write_visual_artifact("permission_with_draft", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_events_tab_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::ToolLifecycle, geometry);
    wait_for_live_startup(&mut helper);
    send_ctrl_tab(helper.writer.as_mut())
        .expect("move review helper off prompt before opening palette");
    send_key(helper.writer.as_mut(), 0x10).expect("open command palette before event log review");
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Commands",
        MARKER_TIMEOUT,
    )
    .expect("wait for command palette before filtering event log review");
    helper
        .writer
        .write_all(b"event")
        .expect("filter event log command");
    helper.writer.flush().expect("flush event log command");
    send_key(helper.writer.as_mut(), b'\r').expect("open event log review surface");

    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Event log",
        MARKER_TIMEOUT,
    )
    .expect("wait for Events surface in narrow geometry");

    assert_screen_contains_all(&screen, &["Event log", "Event details"]);
    write_visual_artifact("events_tab", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_helper_screen_snapshot(
    scenario: HelperScenario,
    geometry: PtyGeometry,
    marker: &str,
) -> String {
    let mut helper = spawn_helper_pty(scenario, geometry);
    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for helper status marker");
    write_visual_artifact(scenario.env_value(), geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn wait_for_live_startup(helper: &mut SpawnedHelper) {
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for live helper startup render");
}

fn show_live_operator_sidebar(helper: &mut SpawnedHelper) {
    send_ctrl_tab(helper.writer.as_mut())
        .expect("focus transcript before opening operator sidebar");
    send_key(helper.writer.as_mut(), b'i').expect("open operator sidebar");
}

fn assert_screen_contains_all(screen: &str, markers: &[&str]) {
    for marker in markers {
        assert!(
            screen.contains(marker),
            "expected screen marker {marker:?}\n{screen}"
        );
    }
}

fn assert_markers_in_order(screen: &str, markers: &[&str]) {
    let mut cursor = 0usize;
    for marker in markers {
        let Some(index) = screen[cursor..].find(marker) else {
            panic!("expected marker {marker:?} after byte {cursor}\n{screen}");
        };
        cursor += index + marker.len();
    }
}

fn send_live_updates(tx: &Sender<LiveUpdate>, events: Vec<EventEnvelopeV1>) {
    for event in events {
        tx.send(LiveUpdate::Event(Box::new(event)))
            .expect("send helper live event");
    }
}

fn spawn_phased_live_updates(tx: Sender<LiveUpdate>, steps: Vec<(Duration, Vec<EventEnvelopeV1>)>) {
    thread::spawn(move || {
        for (delay, events) in steps {
            thread::sleep(delay);
            send_live_updates(&tx, events);
        }
        thread::park();
    });
}

struct SpawnedHelper {
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
}

fn spawn_helper_pty(scenario: HelperScenario, geometry: PtyGeometry) -> SpawnedHelper {
    spawn_helper_pty_with_optional_intent_marker(scenario, geometry, None)
}

fn spawn_helper_pty_with_intent_marker(
    scenario: HelperScenario,
    geometry: PtyGeometry,
    marker_path: &Path,
) -> SpawnedHelper {
    spawn_helper_pty_with_optional_intent_marker(scenario, geometry, Some(marker_path))
}

fn spawn_helper_pty_with_optional_intent_marker(
    scenario: HelperScenario,
    geometry: PtyGeometry,
    marker_path: Option<&Path>,
) -> SpawnedHelper {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(geometry.pty_size())
        .expect("open helper pty pair");

    let current_test_bin = std::env::current_exe().expect("resolve current test binary");
    let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
    command.arg("--exact");
    command.arg(scenario.helper_test_name());
    command.arg("--nocapture");
    command.env("HARNESS_TUI_PTY_HELPER_SCENARIO", scenario.env_value());
    if let Some(marker_path) = marker_path {
        command.env(
            "HARNESS_TUI_REPLAY_INTENT_MARKER",
            marker_path.to_string_lossy().as_ref(),
        );
    }
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn helper test binary");
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .expect("clone helper pty reader");
    let writer = pair.master.take_writer().expect("take helper pty writer");
    let output_rx = spawn_reader_thread(reader);

    SpawnedHelper {
        child,
        writer,
        output_rx,
        parser: geometry.parser(),
    }
}

fn helper_scenario_from_env() -> Option<HelperScenario> {
    match std::env::var("HARNESS_TUI_PTY_HELPER_SCENARIO")
        .ok()
        .as_deref()
    {
        Some("startup_shell") => Some(HelperScenario::StartupShell),
        Some("startup_palette") => Some(HelperScenario::StartupPalette),
        Some("startup_session_history") => Some(HelperScenario::StartupSessionHistory),
        Some("type_first_startup") => Some(HelperScenario::TypeFirstStartup),
        Some("streamed_response") => Some(HelperScenario::StreamedResponse),
        Some("tool_lifecycle") => Some(HelperScenario::ToolLifecycle),
        Some("inline_diff_parity") => Some(HelperScenario::InlineDiffParity),
        Some("permission_with_draft") => Some(HelperScenario::PermissionWithDraft),
        Some("question_permission") => Some(HelperScenario::QuestionPermission),
        Some("details_drawer") => Some(HelperScenario::DetailsDrawer),
        Some("sidebar_session_parity") => Some(HelperScenario::SidebarSessionParity),
        Some("live_orchestration_lifecycle") => Some(HelperScenario::LiveOrchestrationLifecycle),
        Some("live_orchestration_stale_late_result") => {
            Some(HelperScenario::LiveOrchestrationStaleLateResult)
        }
        Some("degraded_bootstrap") => Some(HelperScenario::DegradedBootstrap),
        Some("disconnected_stream") => Some(HelperScenario::DisconnectedStream),
        Some("replay_read_only") => Some(HelperScenario::ReplayReadOnly),
        _ => None,
    }
}

fn replay_intent_marker_path_from_env() -> Option<PathBuf> {
    std::env::var_os("HARNESS_TUI_REPLAY_INTENT_MARKER").map(PathBuf::from)
}

fn assert_or_update_snapshot(name: &str, actual: &str) {
    let path = snapshot_dir().join(format!("{name}.snap"));
    if std::env::var("HARNESS_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        fs::create_dir_all(snapshot_dir()).expect("create snapshot directory");
        fs::write(&path, actual).expect("write updated snapshot file");
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "failed to read snapshot {} ({err}); run with HARNESS_UPDATE_SNAPSHOTS=1 to generate baselines",
            path.display()
        )
    });
    let expected = normalize_snapshot(&expected);
    assert_eq!(
        actual,
        expected,
        "snapshot mismatch for {}; run with HARNESS_UPDATE_SNAPSHOTS=1 to accept changes",
        path.display()
    );
}

fn assert_snapshot_secrets_clean() {
    let dir = snapshot_dir();
    if !dir.exists() {
        return;
    }

    for entry in fs::read_dir(&dir).expect("read snapshot directory") {
        let path = entry.expect("snapshot dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("snap") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read snapshot file");
        assert!(
            !text.contains("sk-"),
            "secret-like token found in snapshot {}",
            path.display()
        );
    }
}

fn normalize_snapshot(input: &str) -> String {
    let normalized = input
        .lines()
        .map(|line| normalize_volatile_line(line.trim_end()))
        .collect::<Vec<_>>()
        .join("\n");
    normalized.trim_end().to_string()
}

fn normalize_volatile_line(line: &str) -> String {
    if let Some(idx) = line.find("Draft preserved · keep") {
        let end = idx + "Draft preserved · keep".len();
        return line[..end].to_string();
    }

    if !line.contains("warn ") {
        if let Some(idx) = line.find("ready for next turn") {
            let end = idx + "ready for next turn".len();
            return line[..end].to_string();
        }
    }

    if !line.contains("warn ") {
        if let Some(idx) = line.find("  ·  agents 0") {
            return line[..idx].to_string();
        }
    }

    let Some(marker_idx) = line.find("Sequences:") else {
        return line.to_string();
    };

    let trailing_border = if line.ends_with('│') { " │" } else { "" };
    format!("{}Sequences: <RANGE>{trailing_border}", &line[..marker_idx])
}

fn wait_for_screen_contains(
    parser: &mut Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = parser.screen().contents();
        if current.contains(needle) {
            return Ok(stabilize_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output disconnected while waiting for '{needle}'; final screen:\n{current}"
                ));
            }
        }
    }
}

fn stabilize_screen(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, initial: String) -> String {
    let mut latest = initial;
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest;
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest,
        }

        let current = parser.screen().contents();
        if current != latest {
            latest = current;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= STABLE_WINDOW {
            return latest;
        }
    }
}

fn drain_output(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn send_ctrl_tab(writer: &mut dyn Write) -> std::io::Result<()> {
    writer.write_all(b"\x1b[9;5u")?;
    writer.flush()
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}

fn terminate_child(mut child: Box<dyn portable_pty::Child + Send>) {
    child.kill().expect("terminate helper tui child");
    std::mem::forget(child);
}

fn configure_deterministic_env(command: &mut CommandBuilder) {
    command.env("HARNESS_DETERMINISTIC", "1");
    command.env("HARNESS_DISABLE_ANIMATIONS", "1");
    command.env("HARNESS_SEED", "42");
    command.env("TERM", "xterm-256color");
    command.env("LANG", "C.UTF-8");
    command.env("LC_ALL", "C.UTF-8");
    command.env("TZ", "UTC");
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
}

fn visual_artifact_root() -> PathBuf {
    if let Ok(dir) = std::env::var("HARNESS_VISUAL_ARTIFACT_DIR") {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return path;
        }
        return repo_root().join(path);
    }

    repo_root().join("target").join("pty-visual-artifacts")
}

fn visual_artifact_path(name: &str, geometry: PtyGeometry) -> PathBuf {
    visual_artifact_root().join(format!(
        "pty_harness_tui_{}_{}x{}.png",
        name, geometry.cols, geometry.rows
    ))
}

fn write_visual_artifact(name: &str, geometry: PtyGeometry, parser: &Parser) {
    let dir = visual_artifact_root();
    fs::create_dir_all(&dir).expect("create PTY visual artifact directory");
    prune_stale_harness_tui_visual_artifacts(&dir).expect("prune stale harness-tui PTY artifacts");
    let image = render_parser_to_image(parser, PTY_RENDER_CONFIG);
    let path = visual_artifact_path(name, geometry);
    image
        .save(&path)
        .unwrap_or_else(|err| panic!("save PTY visual artifact {}: {err}", path.display()));
}

fn assert_visual_artifact_exists(name: &str, geometry: PtyGeometry) {
    let path = visual_artifact_path(name, geometry);
    assert!(
        path.exists(),
        "missing PTY visual artifact: {}",
        path.display()
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-tui should live under <repo>/crates/harness-tui")
}

fn prune_stale_harness_tui_visual_artifacts(visual_dir: &Path) -> Result<(), String> {
    use std::sync::OnceLock;

    static PREPARED: OnceLock<()> = OnceLock::new();
    PREPARED.get_or_init(|| {
        let legacy_dir = visual_dir.join("harness-tui");
        if legacy_dir.exists() {
            fs::remove_dir_all(&legacy_dir).unwrap_or_else(|err| {
                panic!(
                    "remove legacy harness-tui PTY artifact dir {}: {err}",
                    legacy_dir.display()
                )
            });
        }

        for entry in fs::read_dir(visual_dir).unwrap_or_else(|err| {
            panic!(
                "read PTY visual artifacts dir {}: {err}",
                visual_dir.display()
            )
        }) {
            let path = entry.expect("PTY visual artifact dir entry").path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if file_name.starts_with("pty_harness_tui_") && file_name.ends_with(".png") {
                fs::remove_file(&path).unwrap_or_else(|err| {
                    panic!(
                        "remove stale harness-tui PTY artifact {}: {err}",
                        path.display()
                    )
                });
            }
        }
    });

    Ok(())
}

fn startup_session_history_entries() -> Vec<SessionHistoryEntry> {
    vec![
        SessionHistoryEntry {
            run_dir: PathBuf::from("/tmp/sessions/run_resume"),
            catalog: SessionCatalogEntry {
                run_id: "run_resume".to_string(),
                run_name: Some("alpha-run".to_string()),
                status: Some(RunStatus::Finished),
                last_updated_at: Some("2026-03-08T12:34:56Z".to_string()),
                workspace_root: Some("/tmp/workspace".to_string()),
                profile_preset: Some("deep".to_string()),
                provider_model: Some("openai/gpt-5.4-mini".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: true,
                resume_disabled_reason: None,
                artifact_count: 2,
                child_session_count: 1,
                parent_session_id: None,
            },
        },
        SessionHistoryEntry {
            run_dir: PathBuf::from("/tmp/sessions/run_blocked"),
            catalog: SessionCatalogEntry {
                run_id: "run_blocked".to_string(),
                run_name: Some("beta-blocked".to_string()),
                status: Some(RunStatus::Running),
                last_updated_at: Some("2026-03-07T03:21:00Z".to_string()),
                workspace_root: Some("/tmp/workspace".to_string()),
                profile_preset: Some("ops".to_string()),
                provider_model: Some("anthropic/claude-3.7".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: false,
                resume_disabled_reason: Some("run is still active".to_string()),
                artifact_count: 1,
                child_session_count: 0,
                parent_session_id: Some("run_parent".to_string()),
            },
        },
    ]
}

fn find_line_containing(lines: &[&str], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

fn find_line_containing_from(lines: &[&str], start: usize, needle: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}

fn find_line_containing_in_range(
    lines: &[&str],
    start: usize,
    end_exclusive: usize,
    needle: &str,
) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .take(end_exclusive.saturating_sub(start))
        .find_map(|(index, line)| line.contains(needle).then_some(index))
}
