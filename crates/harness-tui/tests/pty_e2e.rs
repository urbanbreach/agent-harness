use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EditAppliedEvent, EditProposedEvent, EventActor, EventEnvelopeV1,
    EventV1, PermissionDecision, PermissionRequestedEvent, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFinishedEvent, RunStartedEvent,
    StaleDetectedEvent, TaskCompletedEvent, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::app::{set_pending_live_launch_metadata, LaunchMetadata, SessionHistoryEntry};
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
const STARTUP_LAUNCHER_READY_MARKER: &str = "Preset worker";

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
    const OPENCODE_EDIT_SIGNOFF: Self = Self {
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
        ready_marker: "Enter send",
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
        marker: "Permission Requested",
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
    PermissionWithDraft,
    DetailsDrawer,
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
            Self::PermissionWithDraft => "permission_with_draft",
            Self::DetailsDrawer => "details_drawer",
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
            Self::PermissionWithDraft => "pty_helper_permission_with_draft",
            Self::DetailsDrawer => "pty_helper_details_drawer",
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
    let status_row = find_line_containing_from(&lines, composer_input, "ready for first turn")
        .expect("status metadata row");
    let footer_row = find_line_containing_from(&lines, composer_input + 1, "Ctrl+p commands")
        .expect("footer legend");

    assert!(composer_input <= status_row);
    assert_eq!(
        footer_row,
        composer_last_row + 1,
        "composer should keep footer hints immediately after the compact shell\n{startup}"
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

    let tool_lifecycle = capture_tool_lifecycle_snapshot(PtyGeometry::OPENCODE_EDIT_SIGNOFF);
    assert_or_update_snapshot("tool_lifecycle", &tool_lifecycle);
    assert_visual_artifact_exists("tool_lifecycle", PtyGeometry::OPENCODE_EDIT_SIGNOFF);

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
fn pty_helper_permission_with_draft() {
    run_helper_if_requested(HelperScenario::PermissionWithDraft);
}

#[test]
fn pty_helper_details_drawer() {
    run_helper_if_requested(HelperScenario::DetailsDrawer);
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

        send_key(helper.writer.as_mut(), b'\t')
            .expect("focus transcript before opening operator sidebar");
        send_key(helper.writer.as_mut(), b'i').expect("open operator sidebar");

        if geometry == PtyGeometry::SIX_WINDOW_DENSE {
            let screen = wait_for_screen_contains(
                &mut helper.parser,
                &helper.output_rx,
                "q quit",
                MARKER_TIMEOUT,
            )
            .expect("wait for dense session shell after sidebar toggle");

            assert!(!screen.contains("Context"));
            assert!(screen.contains("Enter send"));
        } else {
            let screen = wait_for_screen_contains(
                &mut helper.parser,
                &helper.output_rx,
                "Context",
                MARKER_TIMEOUT,
            )
            .expect("wait for operator sidebar markers");

            assert!(screen.contains("Live") || screen.contains("Live · run "));
            assert!(screen.contains("run "));
            assert!(screen.contains("Context"));
        }

        terminate_child(helper.child);
    }
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
        "orch 1a 1q 0r 0s",
        MARKER_TIMEOUT,
    )
    .expect("wait for queued orchestration state");
    assert_screen_contains_all(&queued_screen, &["Todo · 1"]);
    assert!(
        queued_screen.contains("ready for next turn  ·  orch 1a 1q 0r 0s")
            || queued_screen.contains("ready for next turn  ·  orch 1a 0q 1r 0s"),
        "expected queued or just-started orchestration status strip\n{queued_screen}"
    );

    let started_screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "orch 1a 0q 1r 0s",
        MARKER_TIMEOUT,
    )
    .expect("wait for started orchestration state");
    assert_screen_contains_all(
        &started_screen,
        &["Todo · 1", "ready for next turn  ·  orch 1a 0q 1r 0s"],
    );

    let completed_screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Recent context",
        MARKER_TIMEOUT,
    )
    .expect("wait for completed orchestration state");
    assert_screen_contains_all(&completed_screen, &["Context", "ready for next turn"]);
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
        "warn late result after stale",
        MARKER_TIMEOUT,
    )
    .expect("wait for late result orchestration row");
    assert_screen_contains_all(
        &late_result_screen,
        &[
            "Context",
            "ready for next turn  ·  orch 0a 0q 0r 0s · warn late result after stale",
        ],
    );

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

    send_key(helper.writer.as_mut(), b'\t').expect("focus replay details surface");
    send_key(helper.writer.as_mut(), b'\t').expect("attempt replay prompt focus");
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
        assert!(startup_shell.contains("Preset worker"));
        assert!(startup_shell.contains("mock/model-1"));
        assert!(startup_shell.contains("Ask Harness anything…"));
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
                    update_rx: rx,
                },
                exit_on_finish: false,
                on_ui_intent,
                keybindings: None,
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
            update_rx: rx,
        },
        exit_on_finish: false,
        on_ui_intent,
        keybindings: None,
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
        let UiIntent::SubmitPrompt { text } = intent else {
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
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_read".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs","start_line":1,"limit":24}"#.to_string(),
                args_digest: "digest-tool-lifecycle-read-args".to_string(),
                metadata: None,
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
                metadata: None,
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
                tool_call_id: "tc_shell".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"cargo test -p harness-tui","cwd":"/workspace"}"#
                    .to_string(),
                args_digest: "digest-tool-lifecycle-shell-args".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            12,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_shell".to_string(),
            }),
        ),
        envelope(
            13,
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
            14,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.to_string(),
                delta: "Tool summaries are now easier to scan, and edits stay inline.".to_string(),
            }),
        ),
        envelope(
            15,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-tool-lifecycle-response".to_string()),
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
    .expect("write tool lifecycle diff fixture");
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
        "Command palette",
        MARKER_TIMEOUT,
    )
    .expect("wait for startup command palette");

    write_visual_artifact("startup_palette", geometry, &helper.parser);
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
        "Command palette",
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

    write_visual_artifact("streamed_response", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_tool_lifecycle_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::ToolLifecycle, geometry);
    let screen = wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "← Patched crates/harness-tui/src/ui.rs",
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
        screen.contains(PRESERVED_DRAFT_TEXT),
        "permission snapshot lost draft"
    );
    write_visual_artifact("permission_with_draft", geometry, &helper.parser);
    terminate_child(helper.child);
    normalize_snapshot(&screen)
}

fn capture_events_tab_snapshot(geometry: PtyGeometry) -> String {
    let mut helper = spawn_helper_pty(HelperScenario::ToolLifecycle, geometry);
    wait_for_live_startup(&mut helper);
    send_key(helper.writer.as_mut(), b'\t')
        .expect("move review helper off prompt before opening palette");
    send_key(helper.writer.as_mut(), 0x10).expect("open command palette before event log review");
    wait_for_screen_contains(
        &mut helper.parser,
        &helper.output_rx,
        "Command palette",
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
    send_key(helper.writer.as_mut(), b'\t')
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
        Some("permission_with_draft") => Some(HelperScenario::PermissionWithDraft),
        Some("details_drawer") => Some(HelperScenario::DetailsDrawer),
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
                provider_model: Some("openai/gpt-5.4".to_string()),
                mode_source: SessionModeSource::InteractiveLive,
                is_resumable: true,
                resume_disabled_reason: None,
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
