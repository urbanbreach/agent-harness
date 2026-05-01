use image::{GenericImage, Rgb, RgbImage};
use portable_pty::{CommandBuilder, PtySize};
use serde_json::{json, Value};
use std::cmp;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver, RecvTimeoutError},
    OnceLock,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vt100::Parser as VtParser;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "support/markers.rs"]
mod markers;
#[path = "support/pty_process.rs"]
mod pty_process;
#[path = "support/visual_contracts.rs"]
mod visual_contracts;
#[path = "support/visual_renderer.rs"]
mod visual_renderer;

use markers::{
    LIVE_OPERATOR_EMPTY_MARKER, LIVE_READY_NEXT_TURN_MARKER, LIVE_SUCCESS_COMPOSER_MARKER,
    OPERATOR_FILES_MARKER, REPLAY_DENSE_READY_MARKER, REPLAY_READY_MARKER,
    RUN_FINISHED_SHELL_MARKERS, STARTUP_COMMAND_PALETTE_MARKER, STARTUP_CONTINUE_HISTORY_MARKER,
    STARTUP_CONTINUE_HISTORY_READY_MARKER, STARTUP_HOME_COMPOSER_HINT_MARKER,
    STARTUP_HOME_DENSE_VALUE_PROP_MARKER, STARTUP_HOME_SHORTCUT_MARKER,
    STARTUP_LAUNCHER_READY_MARKER, STARTUP_REPLAY_HISTORY_MARKER,
};
use pty_process::{spawn_pty_process, SpawnedPtyProcess};
use visual_contracts::OFFLINE_VISUAL_EVIDENCE_CONTRACTS;

use visual_renderer::{
    extract_region_pixels, extract_region_render_state, parse_ttf_antialias_env,
    render_parser_to_image, TerminalRenderConfig,
};

const VISUAL_MANIFEST_JSON_FILE: &str = "manifest.json";
const VISUAL_MANIFEST_JSONL_FILE: &str = "manifest.jsonl";

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(6);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const STABLE_WINDOW: Duration = Duration::from_millis(180);
const STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const PRIMARY_SIGNOFF_COLS: u16 = 160;
const PRIMARY_SIGNOFF_ROWS: u16 = 48;
const CELL_WIDTH: u32 = 16;
const CELL_HEIGHT: u32 = 30;
const PTY_RENDER_CONFIG: TerminalRenderConfig = TerminalRenderConfig::new(CELL_WIDTH, CELL_HEIGHT);

#[test]
fn pty_e2e_permission_dock_parity() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::MINIMUM_SIGNOFF, &session_dir, |command| {
        command.arg("--scenario");
        command.arg("golden_path_interactive");
        command.env(
            "HARNESS_TUI_PENDING_LIVE_PROMPT_DRAFT",
            LIVE_STATE_FIXTURES.permission.draft_text,
        );
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        LIVE_STATE_FIXTURES.permission.marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for permission marker");

    send_key(harness.writer.as_mut(), 0x10).expect("attempt opening palette under permission");
    send_key(harness.writer.as_mut(), b'/').expect("attempt opening slash under permission");

    let current_screen = harness.parser.screen().contents();
    let stable_screen = stabilize_screen(&mut harness.parser, &harness.output_rx, current_screen);
    let permission_visual = capture_manifest_backed_visual_checkpoint(
        "permission",
        "permission_dock_parity",
        &harness.parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.permission.focus_capture,
        &[
            LIVE_STATE_FIXTURES.permission.marker,
            LIVE_STATE_FIXTURES.permission.draft_text,
        ],
    )
    .expect("capture permission parity checkpoint image");

    assert!(stable_screen.contains(LIVE_STATE_FIXTURES.permission.marker));
    assert!(stable_screen.contains(LIVE_STATE_FIXTURES.permission.draft_text));
    assert!(!stable_screen.contains("Permission blocked"));
    assert!(!stable_screen.contains(STARTUP_COMMAND_PALETTE_MARKER));
    assert!(visual_dir.join(&permission_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_permission_dock_parity",
        checkpoint_visual_snapshot(
            &stable_screen,
            &[
                LIVE_STATE_FIXTURES.permission.marker,
                LIVE_STATE_FIXTURES.permission.draft_text,
            ],
            &permission_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate permission overlay parity harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_helper_permission_with_draft() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let mut harness = spawn_harness_tui(PtyGeometry::MINIMUM_SIGNOFF, &session_dir, |command| {
        command.arg("--scenario");
        command.arg("golden_path_interactive");
        command.env(
            "HARNESS_TUI_PENDING_LIVE_PROMPT_DRAFT",
            LIVE_STATE_FIXTURES.permission.draft_text,
        );
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        LIVE_STATE_FIXTURES.permission.marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for permission marker");

    send_key(harness.writer.as_mut(), 0x10).expect("attempt opening palette under permission");
    send_key(harness.writer.as_mut(), b'/').expect("attempt opening slash under permission");

    let current_screen = harness.parser.screen().contents();
    let stable_screen = stabilize_screen(&mut harness.parser, &harness.output_rx, current_screen);

    assert!(stable_screen.contains(LIVE_STATE_FIXTURES.permission.marker));
    assert!(stable_screen.contains(LIVE_STATE_FIXTURES.permission.draft_text));
    assert!(!stable_screen.contains("Permission blocked"));
    assert!(!stable_screen.contains(STARTUP_COMMAND_PALETTE_MARKER));

    harness
        .child
        .kill()
        .expect("terminate permission overlay helper harness");
    std::mem::forget(harness.child);
}

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
        cols: PRIMARY_SIGNOFF_COLS,
        rows: PRIMARY_SIGNOFF_ROWS,
    };

    fn pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn parser(self) -> VtParser {
        VtParser::new(self.rows, self.cols, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PromptFixture {
    ready_marker: &'static str,
    focus_keys: &'static [u8],
}

impl PromptFixture {
    const LIVE_COMPOSER: Self = Self {
        ready_marker: STARTUP_HOME_COMPOSER_HINT_MARKER,
        focus_keys: b"",
    };

    fn focus_prompt(self, writer: &mut dyn Write) -> std::io::Result<()> {
        for key in self.focus_keys {
            send_key(writer, *key)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DraftFixture {
    text: &'static str,
    response_marker: &'static str,
    submit_key: u8,
    draft_focus_capture: FocusCapture,
    draft_snapshot_markers: &'static [&'static str],
    focus_capture: FocusCapture,
    snapshot_markers: &'static [&'static str],
}

impl DraftFixture {
    const HELLO_WORLD: Self = Self {
        text: "Hello from PTY",
        response_marker: "Hello world",
        submit_key: b'\r',
        draft_focus_capture: FocusCapture::anchored("Hello from PTY", 24, 4),
        draft_snapshot_markers: &["Hello from PTY", STARTUP_HOME_SHORTCUT_MARKER],
        focus_capture: FocusCapture::anchored("Hello world", 28, 6),
        snapshot_markers: &["Hello world", LIVE_READY_NEXT_TURN_MARKER],
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
    draft_text: &'static str,
    approve_key: u8,
    follow_toggle_key: u8,
    rewind_key: u8,
    rewind_steps: usize,
    focus_capture: FocusCapture,
    snapshot_markers: &'static [&'static str],
}

impl PermissionFixture {
    const TOOL_CALL: Self = Self {
        marker: "Permission required",
        draft_text: "keep this draft",
        approve_key: 0x19,
        follow_toggle_key: b' ',
        rewind_key: b'k',
        rewind_steps: 64,
        focus_capture: FocusCapture::anchored_exact("Permission required", 24, 1),
        snapshot_markers: &["Permission required", "keep this draft", "Allow once"],
    };

    fn write_preserved_draft(self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.draft_text.as_bytes())?;
        writer.flush()
    }

    fn approve(self, writer: &mut dyn Write) -> std::io::Result<()> {
        send_key(writer, self.approve_key)
    }

    fn toggle_follow(self, writer: &mut dyn Write) -> std::io::Result<()> {
        send_key(writer, self.follow_toggle_key)
    }

    fn rewind_to_first_activity(self, writer: &mut dyn Write) -> std::io::Result<()> {
        for _ in 0..self.rewind_steps {
            send_key(writer, self.rewind_key)?;
        }
        Ok(())
    }

    fn prepare_stable_capture(self, writer: &mut dyn Write) -> std::io::Result<()> {
        self.approve(writer)?;
        self.toggle_follow(writer)?;
        self.rewind_to_first_activity(writer)
    }
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

type SpawnedHarnessPty = SpawnedPtyProcess;

fn spawn_harness_tui(
    geometry: PtyGeometry,
    session_dir: &Path,
    configure: impl FnOnce(&mut CommandBuilder),
) -> SpawnedHarnessPty {
    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    configure(&mut command);
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(repo_root);
    configure_deterministic_env(&mut command);

    let mut process = spawn_pty_process(geometry.pty_size(), command, "harness tui")
        .expect("spawn harness tui command");
    process.parser = geometry.parser();
    process
}

fn open_startup_command_palette(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
) -> String {
    send_key(writer, 0x10).expect("open startup command palette with Ctrl+P");
    wait_for_screen_contains(
        parser,
        output_rx,
        STARTUP_COMMAND_PALETTE_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for startup command palette")
}

fn open_startup_session_history(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    command_name: &str,
    overlay_marker: &str,
) -> String {
    open_startup_command_palette(parser, output_rx, writer);
    writer
        .write_all(command_name.as_bytes())
        .expect("type startup launcher palette command");
    writer
        .flush()
        .expect("flush startup launcher palette command");
    send_key(writer, b'\r').expect("open startup session history overlay");
    wait_for_screen_contains(parser, output_rx, overlay_marker, MARKER_TIMEOUT)
        .expect("wait for startup session history overlay")
}

fn move_list_selection(writer: &mut dyn Write, steps: usize) {
    for _ in 0..steps {
        writer
            .write_all(b"\x1b[B")
            .expect("write down-arrow escape sequence");
        writer.flush().expect("flush down-arrow escape sequence");
    }
}

fn show_live_operator_sidebar(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    marker: &str,
) -> String {
    send_key(writer, b'\t').expect("focus transcript before opening operator sidebar");
    send_key(writer, b'i').expect("toggle live operator sidebar");
    wait_for_screen_contains(parser, output_rx, marker, MARKER_TIMEOUT)
        .expect("wait for live operator sidebar")
}

fn spawn_resumed_quiescent_session(
    geometry: PtyGeometry,
    session_dir: &Path,
    config_path: &Path,
    _run_id: &str,
) -> SpawnedHarnessPty {
    let mut harness = spawn_harness_tui(geometry, session_dir, |command| {
        command.arg("--config");
        command.arg(config_path.to_string_lossy().to_string());
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup home before continuing quiescent session");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "resume",
        "continue ready",
    );
    send_key(harness.writer.as_mut(), b'\r').expect("select quiescent session from history");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        LIVE_READY_NEXT_TURN_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for continued live session shell");

    harness
}

#[test]
fn pty_e2e_startup_home_primary() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    let screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for compose-first startup home shell");
    let screen = stabilize_screen(&mut harness.parser, &harness.output_rx, screen);
    let visual = capture_manifest_backed_visual_checkpoint(
        "startup_shell",
        "startup_home_primary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_HOME_COMPOSER_HINT_MARKER, 48, 8),
        &[
            STARTUP_HOME_SHORTCUT_MARKER,
            STARTUP_HOME_COMPOSER_HINT_MARKER,
        ],
    )
    .expect("capture startup home primary image");

    assert!(screen.contains(STARTUP_HOME_SHORTCUT_MARKER));
    assert!(screen.contains(STARTUP_HOME_COMPOSER_HINT_MARKER));
    assert!(screen.contains("Worker model-1 mock · Demo"));
    assert!(!screen.contains("New session"));
    assert!(!screen.contains("Continue session"));
    assert!(!screen.contains("Replay session"));
    assert!(visual_dir.join(&visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_home_primary",
        checkpoint_visual_snapshot(
            &screen,
            &[
                STARTUP_HOME_SHORTCUT_MARKER,
                STARTUP_HOME_COMPOSER_HINT_MARKER,
            ],
            &visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate startup home primary harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_startup_home_dense() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::SIX_WINDOW_DENSE, &session_dir, |command| {
        command.arg("--mock");
    });

    let screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_DENSE_VALUE_PROP_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for dense compose-first startup home shell");
    let screen = stabilize_screen(&mut harness.parser, &harness.output_rx, screen);
    let visual = capture_manifest_backed_visual_checkpoint(
        "startup_shell",
        "startup_home_dense",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_HOME_DENSE_VALUE_PROP_MARKER, 32, 6),
        &[STARTUP_HOME_DENSE_VALUE_PROP_MARKER],
    )
    .expect("capture startup home dense image");

    assert!(screen.contains(STARTUP_HOME_COMPOSER_HINT_MARKER));
    assert!(screen.contains("Worker model-1 mock · Demo"));
    assert!(screen.contains(STARTUP_HOME_DENSE_VALUE_PROP_MARKER));
    assert!(!screen.contains("New session"));
    assert!(!screen.contains("Continue session"));
    assert!(!screen.contains("Replay session"));
    assert!(visual_dir.join(&visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_home_dense",
        checkpoint_visual_snapshot(&screen, &[STARTUP_HOME_DENSE_VALUE_PROP_MARKER], &visual)
    );

    harness
        .child
        .kill()
        .expect("terminate startup home dense harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_session_shell_primary() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut live = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });
    wait_for_screen_contains(
        &mut live.parser,
        &live.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup home before opening primary session shell");

    type_text(live.writer.as_mut(), "shell parity task");
    send_key(live.writer.as_mut(), b'\r').expect("submit mock prompt for primary live shell");
    let live_screen = wait_for_screen_contains(
        &mut live.parser,
        &live.output_rx,
        LIVE_READY_NEXT_TURN_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for wide live session shell sidebar");
    let live_visual = capture_manifest_backed_visual_checkpoint(
        "live_shell",
        "session_shell_primary_live",
        &live.parser,
        &visual_dir,
        FocusCapture::anchored_exact(OPERATOR_FILES_MARKER, 20, 8),
        &[
            "shell parity task",
            "Shell parity looks good.",
            OPERATOR_FILES_MARKER,
            LIVE_OPERATOR_EMPTY_MARKER,
            LIVE_SUCCESS_COMPOSER_MARKER,
            LIVE_READY_NEXT_TURN_MARKER,
        ],
    )
    .expect("capture primary live session shell image");

    assert!(live_screen.contains("shell parity task"));
    assert!(live_screen.contains("Shell parity looks good."));
    assert!(live_screen.contains(OPERATOR_FILES_MARKER));
    assert!(live_screen.contains(LIVE_OPERATOR_EMPTY_MARKER));
    assert!(live_screen.contains(LIVE_SUCCESS_COMPOSER_MARKER));
    assert!(live_screen.contains(LIVE_READY_NEXT_TURN_MARKER));
    assert!(!live_screen.contains("Cancelled"));
    assert!(!live_screen.contains("mock fixture missing"));
    assert!(!live_screen.contains("Tabs"));
    assert!(visual_dir.join(&live_visual.file_name).exists());
    assert!(live_visual.manifest_json_path.exists());
    assert!(live_visual.manifest_jsonl_path.exists());

    live.child
        .kill()
        .expect("terminate primary live session shell harness");
    std::mem::forget(live.child);

    let mut replay = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });
    wait_for_screen_contains(
        &mut replay.parser,
        &replay.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup home before opening replay session shell");

    open_startup_session_history(
        &mut replay.parser,
        &replay.output_rx,
        replay.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(replay.writer.as_mut(), b'\r').expect("select replay session from startup history");
    let replay_screen = wait_for_screen_contains(
        &mut replay.parser,
        &replay.output_rx,
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay session shell read-only composer");
    let replay_visual = capture_manifest_backed_visual_checkpoint(
        "replay_shell",
        "session_shell_primary_replay",
        &replay.parser,
        &visual_dir,
        FocusCapture::anchored_exact(REPLAY_DENSE_READY_MARKER, 20, 2),
        &[
            REPLAY_DENSE_READY_MARKER,
            "shell parity task",
            OPERATOR_FILES_MARKER,
            LIVE_OPERATOR_EMPTY_MARKER,
            "Replay is read-only.",
        ],
    )
    .expect("capture primary replay session shell image");

    assert!(replay_screen.contains(REPLAY_DENSE_READY_MARKER));
    assert!(replay_screen.contains("shell parity task"));
    assert!(replay_screen.contains(OPERATOR_FILES_MARKER));
    assert!(replay_screen.contains(LIVE_OPERATOR_EMPTY_MARKER));
    assert!(replay_screen.contains("Replay is read-only."));
    assert!(!replay_screen.contains("Tabs"));
    assert!(visual_dir.join(&replay_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_session_shell_primary_replay",
        checkpoint_visual_snapshot(
            &replay_screen,
            &[
                REPLAY_DENSE_READY_MARKER,
                "shell parity task",
                OPERATOR_FILES_MARKER,
                LIVE_OPERATOR_EMPTY_MARKER,
                "Replay is read-only.",
            ],
            &replay_visual,
        )
    );

    replay
        .child
        .kill()
        .expect("terminate primary replay session shell harness");
    std::mem::forget(replay.child);
}

#[test]
fn native_tool_parity_pty_lane() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");
    let run_id = "run_native_tool_parity";
    write_native_tool_parity_fixture(&session_dir, run_id);

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup home before opening rich transcript session");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(harness.writer.as_mut(), b'\r')
        .expect("select native tool parity replay session from history");

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for native tool parity replay shell");

    let current = harness.parser.screen().contents();
    let screen = stabilize_screen(&mut harness.parser, &harness.output_rx, current);

    let thinking_visual = capture_manifest_backed_visual_checkpoint(
        "transcript_shell",
        "native_tool_parity_thinking",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Thinking: Drafting the inline parity pass.", 40, 5),
        &[
            "Thinking: Drafting the inline parity pass.",
            "Inline transcript parity is easier to scan now.",
            "Bring native tool parity inline",
            REPLAY_DENSE_READY_MARKER,
        ],
    )
    .expect("capture native tool parity thinking image");

    let task_visual = capture_manifest_backed_visual_checkpoint(
        "transcript_shell",
        "native_tool_parity_task_row",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(
            "task audit transcript parity [subagent_type=researcher]",
            48,
            6,
        ),
        &[
            "Bring native tool parity inline",
            "Gathered context · 2 reads",
            "task audit transcript parity [subagent_type=researcher]",
            "webfetch https://example.test/report.pdf [format=markdown]",
            "docs-rs_search_in_crate Layout [crate_name=ratatui]",
        ],
    )
    .expect("capture native tool parity task-row image");
    let fetch_visual = capture_manifest_backed_visual_checkpoint(
        "transcript_shell",
        "native_tool_parity_fetch_row",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(
            "webfetch https://example.test/report.pdf [format=markdown]",
            44,
            6,
        ),
        &[
            "webfetch https://example.test/report.pdf [format=markdown]",
            "# Run harness-tui tests",
            "$ cargo test -p harness-tui",
            "exit code: 1",
            "stderr: snapshot mismatch",
        ],
    )
    .expect("capture native tool parity fetch-row image");
    let mcp_visual = capture_manifest_backed_visual_checkpoint(
        "transcript_shell",
        "native_tool_parity_mcp_background",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("docs-rs_search_in_crate Layout [crate_name=ratatui]", 44, 6),
        &[
            "docs-rs_search_in_crate Layout [crate_name=ratatui]",
            REPLAY_DENSE_READY_MARKER,
        ],
    )
    .expect("capture native tool parity MCP background image");

    assert!(screen.contains("Bring native tool parity inline"));
    assert!(screen.contains("Thinking: Drafting the inline parity pass."));
    assert!(screen.contains("Inline transcript parity is easier to scan now."));
    assert!(screen.contains("Gathered context · 2 reads"));
    assert!(screen.contains("task audit transcript parity [subagent_type=researcher]"));
    assert!(screen.contains("webfetch https://example.test/report.pdf [format=markdown]"));
    assert!(screen.contains("docs-rs_search_in_crate Layout [crate_name=ratatui]"));
    assert!(!screen.contains("struct Layout"));
    assert!(!screen.contains("module layout"));
    assert!(screen.contains("# Run harness-tui tests"));
    assert!(screen.contains("$ cargo test -p harness-tui"));
    assert!(screen.contains("exit code: 1"));
    assert!(screen.contains("stderr: snapshot mismatch"));
    assert!(screen.contains(REPLAY_DENSE_READY_MARKER));
    assert!(!screen.contains("Event log"));
    assert!(!screen.contains("Keyboard Shortcuts:"));
    assert!(visual_dir.join(&thinking_visual.file_name).exists());
    assert!(visual_dir.join(&task_visual.file_name).exists());
    assert!(visual_dir.join(&fetch_visual.file_name).exists());
    assert!(visual_dir.join(&mcp_visual.file_name).exists());
    insta::assert_snapshot!(
        "native_tool_parity_thinking",
        checkpoint_visual_snapshot(
            &screen,
            &[
                "Thinking: Drafting the inline parity pass.",
                "Inline transcript parity is easier to scan now.",
                "Bring native tool parity inline",
                REPLAY_DENSE_READY_MARKER,
            ],
            &thinking_visual,
        )
    );
    insta::assert_snapshot!(
        "native_tool_parity_task_row",
        checkpoint_visual_snapshot(
            &screen,
            &[
                "Bring native tool parity inline",
                "Gathered context · 2 reads",
                "task audit transcript parity [subagent_type=researcher]",
                "webfetch https://example.test/report.pdf [format=markdown]",
                "docs-rs_search_in_crate Layout [crate_name=ratatui]",
            ],
            &task_visual,
        )
    );
    insta::assert_snapshot!(
        "native_tool_parity_fetch_row",
        checkpoint_visual_snapshot(
            &screen,
            &[
                "webfetch https://example.test/report.pdf [format=markdown]",
                "# Run harness-tui tests",
                "$ cargo test -p harness-tui",
                "exit code: 1",
                "stderr: snapshot mismatch",
            ],
            &fetch_visual,
        )
    );
    insta::assert_snapshot!(
        "native_tool_parity_mcp_background",
        checkpoint_visual_snapshot(
            &screen,
            &[
                "docs-rs_search_in_crate Layout [crate_name=ratatui]",
                REPLAY_DENSE_READY_MARKER,
            ],
            &mcp_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate native tool parity session harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_dense_tool_log_parity() {
    native_tool_parity_pty_lane();
}

async fn start_responses_wiremock_server() -> MockServer {
    let server = MockServer::start().await;
    let sse_body = responses_api_sse_fixture();
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_body.clone(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(response_template.clone())
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(response_template)
        .mount(&server)
        .await;

    server
}

fn type_text(writer: &mut dyn Write, text: &str) {
    writer
        .write_all(text.as_bytes())
        .expect("type text into PTY");
    writer.flush().expect("flush PTY text");
}

fn quit_startup_tui(parser: &mut VtParser, output_rx: &Receiver<Vec<u8>>, writer: &mut dyn Write) {
    open_startup_command_palette(parser, output_rx, writer);
    type_text(writer, "quit");
    send_key(writer, b'\r').expect("confirm quit from startup palette");
}

fn wait_for_process_exit(
    child: &mut Box<dyn portable_pty::Child + Send>,
    output_rx: &Receiver<Vec<u8>>,
    parser: &mut VtParser,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        match child
            .try_wait()
            .map_err(|err| format!("failed to poll harness PTY child: {err}"))?
        {
            Some(status) if status.success() => return Ok(()),
            Some(status) => {
                return Err(format!(
                    "harness PTY child exited with status {:?}; final screen:\n{}",
                    status.exit_code(),
                    screen_contents(parser)
                ));
            }
            None => {}
        }

        let now = Instant::now();
        if now >= deadline {
            child
                .kill()
                .map_err(|err| format!("failed to kill timed out PTY child: {err}"))?;
            return Err(format!(
                "timed out waiting for PTY child to exit after {timeout:?}; final screen:\n{}",
                screen_contents(parser)
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_lifecycle_shell_flow() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let scenario_session_dir = create_temp_session_dir();
    let mut scenario_harness = spawn_harness_tui(
        PtyGeometry::MINIMUM_SIGNOFF,
        &scenario_session_dir,
        |command| {
            command.arg("--scenario");
            command.arg("golden_path_interactive");
            command.env(
                "HARNESS_TUI_PENDING_LIVE_PROMPT_DRAFT",
                LIVE_STATE_FIXTURES.permission.draft_text,
            );
        },
    );

    let permission_checkpoint = wait_for_screen_contains(
        &mut scenario_harness.parser,
        &scenario_harness.output_rx,
        LIVE_STATE_FIXTURES.permission.marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for permission marker");
    let permission_visual = capture_visual_checkpoint(
        "permission_requested",
        &scenario_harness.parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.permission.focus_capture,
    )
    .expect("capture permission checkpoint image");
    assert!(
        permission_checkpoint.contains(LIVE_STATE_FIXTURES.permission.draft_text),
        "permission checkpoint should preserve the draft under the overlay"
    );
    assert!(visual_dir.join(&permission_visual.file_name).exists());

    LIVE_STATE_FIXTURES
        .permission
        .approve(scenario_harness.writer.as_mut())
        .expect("send approve key");

    let inline_completion_checkpoint = wait_for_screen_contains(
        &mut scenario_harness.parser,
        &scenario_harness.output_rx,
        LIVE_READY_NEXT_TURN_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for ready-for-next-turn shell marker");
    let inline_completion_visual = capture_manifest_backed_visual_checkpoint(
        "live_shell",
        "inline_completion_shell",
        &scenario_harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(LIVE_READY_NEXT_TURN_MARKER, 20, 1),
        RUN_FINISHED_SHELL_MARKERS,
    )
    .expect("capture inline completion shell checkpoint image");
    insta::assert_snapshot!(
        "pty_inline_completion_shell",
        checkpoint_visual_snapshot(
            &inline_completion_checkpoint,
            RUN_FINISHED_SHELL_MARKERS,
            &inline_completion_visual
        )
    );

    scenario_harness
        .child
        .kill()
        .expect("terminate scenario lifecycle handoff after signoff");
    std::mem::forget(scenario_harness.child);
}

#[test]
fn pty_e2e_startup_command_palette() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup compose-first marker");

    let palette_screen = open_startup_command_palette(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
    );
    let palette_visual = capture_manifest_backed_visual_checkpoint(
        "startup_command_palette",
        "startup_command_palette",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_COMMAND_PALETTE_MARKER, 56, 9),
        &[
            STARTUP_COMMAND_PALETTE_MARKER,
            "New session",
            STARTUP_CONTINUE_HISTORY_MARKER,
            STARTUP_REPLAY_HISTORY_MARKER,
        ],
    )
    .expect("capture startup command palette image");
    assert!(visual_dir.join(&palette_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_command_palette",
        checkpoint_visual_snapshot(
            &palette_screen,
            &[
                STARTUP_COMMAND_PALETTE_MARKER,
                "New session",
                STARTUP_CONTINUE_HISTORY_MARKER,
                STARTUP_REPLAY_HISTORY_MARKER,
            ],
            &palette_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate startup command palette harness child");
    std::mem::forget(harness.child);
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_tui_interactive_prompt_streams_response() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = MockServer::start().await;
    let sse_body = responses_api_sse_fixture();
    let response_template = ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_raw(sse_body.clone(), "text/event-stream");

    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(response_template.clone())
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(response_template)
        .mount(&server)
        .await;

    let session_dir = create_temp_session_dir();
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
    let geometry = PtyGeometry::MINIMUM_SIGNOFF;

    let mut harness = spawn_harness_tui(geometry, &session_dir, |command| {
        command.arg("--config");
        command.arg(config_path.to_string_lossy().to_string());
    });
    let mut writer = harness.writer;
    let output_rx = harness.output_rx;
    let mut parser = harness.parser;
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    wait_for_screen_contains(
        &mut parser,
        &output_rx,
        LIVE_STATE_FIXTURES.prompt.ready_marker,
        STARTUP_TIMEOUT,
    )
    .expect("wait for initial TUI render");

    LIVE_STATE_FIXTURES
        .prompt
        .focus_prompt(writer.as_mut())
        .expect("focus prompt pane");
    LIVE_STATE_FIXTURES
        .draft
        .write(writer.as_mut())
        .expect("type reusable startup draft fixture");

    let startup_checkpoint = wait_for_screen_contains(
        &mut parser,
        &output_rx,
        LIVE_STATE_FIXTURES.draft.text,
        MARKER_TIMEOUT,
    )
    .expect("wait for type-first startup draft marker");
    let startup_visual = capture_manifest_backed_visual_checkpoint(
        "live_shell",
        "interactive_type_first_startup",
        &parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.draft.draft_focus_capture,
        LIVE_STATE_FIXTURES.draft.draft_snapshot_markers,
    )
    .expect("capture interactive startup checkpoint image");
    insta::assert_snapshot!(
        "pty_interactive_type_first_startup",
        checkpoint_visual_snapshot(
            &startup_checkpoint,
            LIVE_STATE_FIXTURES.draft.draft_snapshot_markers,
            &startup_visual
        )
    );

    LIVE_STATE_FIXTURES
        .draft
        .submit(writer.as_mut())
        .expect("submit reusable draft fixture");

    let prompt_checkpoint = wait_for_screen_contains(
        &mut parser,
        &output_rx,
        LIVE_STATE_FIXTURES.draft.response_marker,
        MARKER_TIMEOUT,
    )
    .expect("wait for streamed response text marker");
    let prompt_visual = capture_manifest_backed_visual_checkpoint(
        "live_shell",
        "interactive_prompt_stream",
        &parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.draft.focus_capture,
        LIVE_STATE_FIXTURES.draft.snapshot_markers,
    )
    .expect("capture interactive prompt checkpoint image");
    insta::assert_snapshot!(
        "pty_interactive_prompt_stream",
        checkpoint_visual_snapshot(
            &prompt_checkpoint,
            LIVE_STATE_FIXTURES.draft.snapshot_markers,
            &prompt_visual
        )
    );

    drop(writer);

    harness
        .child
        .kill()
        .expect("terminate interactive tui child");
    std::mem::forget(harness.child);
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_continue_quiescent_session() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;

    let session_dir = create_temp_session_dir();
    let run_id = "run_resume_quiescent";
    let run_dir = write_quiescent_resume_fixture(&session_dir, run_id);
    let events_path = run_dir.join("events.jsonl");
    let before_events = load_events_jsonl(&events_path);
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--config");
        command.arg(config_path.to_string_lossy().to_string());
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher ready");
    let startup_visual = capture_visual_checkpoint(
        "startup_launcher_ready",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_LAUNCHER_READY_MARKER, 40, 2),
    )
    .expect("capture startup launcher checkpoint image");
    assert!(visual_dir.join(&startup_visual.file_name).exists());

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        "interactive and prompt runs stay available",
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup replay history scope line");
    let replay_history_focus = FocusCapture::anchored_exact(STARTUP_REPLAY_HISTORY_MARKER, 28, 2);
    let replay_history_screen = stabilize_focus_render_state(
        &mut harness.parser,
        &harness.output_rx,
        replay_history_focus,
    );
    let replay_history_visual = capture_manifest_backed_visual_checkpoint(
        "startup_session_history",
        "startup_replay_history",
        &harness.parser,
        &visual_dir,
        replay_history_focus,
        &[
            STARTUP_REPLAY_HISTORY_MARKER,
            "Read-only replays",
            "interactive and prompt runs stay available",
        ],
    )
    .expect("capture startup replay history image");
    assert!(visual_dir.join(&replay_history_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_replay_history",
        checkpoint_visual_snapshot_without_focus_hash(
            &replay_history_screen,
            &[
                STARTUP_REPLAY_HISTORY_MARKER,
                "Read-only replays",
                "interactive and prompt runs stay available",
            ],
            &replay_history_visual,
        )
    );
    send_key(harness.writer.as_mut(), 0x1b).expect("close replay history overlay");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for startup launcher after closing replay history overlay");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "resume",
        STARTUP_CONTINUE_HISTORY_READY_MARKER,
    );
    let history_focus = FocusCapture::anchored_exact(STARTUP_CONTINUE_HISTORY_READY_MARKER, 28, 3);
    let history_screen =
        stabilize_focus_render_state(&mut harness.parser, &harness.output_rx, history_focus);
    let history_visual = capture_manifest_backed_visual_checkpoint(
        "startup_session_history",
        "startup_continue_history",
        &harness.parser,
        &visual_dir,
        history_focus,
        &[
            STARTUP_CONTINUE_HISTORY_MARKER,
            STARTUP_CONTINUE_HISTORY_READY_MARKER,
            "Interactive histories",
        ],
    )
    .expect("capture startup resume history image");
    assert!(visual_dir.join(&history_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_continue_history",
        checkpoint_visual_snapshot_without_focus_hash(
            &history_screen,
            &[
                STARTUP_CONTINUE_HISTORY_MARKER,
                STARTUP_CONTINUE_HISTORY_READY_MARKER,
                "Interactive histories",
            ],
            &history_visual,
        )
    );

    send_key(harness.writer.as_mut(), b'\r').expect("continue selected quiescent session");
    let continued_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        LIVE_READY_NEXT_TURN_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for continued session shell after reopening run");
    let continued_visual = capture_manifest_backed_visual_checkpoint(
        "continue_session",
        "continue_quiescent_session",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(LIVE_READY_NEXT_TURN_MARKER, 20, 1),
        &[
            "historical answer",
            LIVE_OPERATOR_EMPTY_MARKER,
            LIVE_READY_NEXT_TURN_MARKER,
        ],
    )
    .expect("capture continued session checkpoint image");
    insta::assert_snapshot!(
        "pty_continue_quiescent_session",
        checkpoint_visual_snapshot(
            &continued_screen,
            &[
                "historical answer",
                LIVE_OPERATOR_EMPTY_MARKER,
                LIVE_READY_NEXT_TURN_MARKER,
            ],
            &continued_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate continued-session launcher after coverage");
    std::mem::forget(harness.child);

    let after_events = load_events_jsonl(&events_path);
    assert!(
        after_events.len() > before_events.len(),
        "continued session should append events to the existing run"
    );
    assert!(
        after_events.iter().all(|event| {
            event
                .get("run_id")
                .and_then(Value::as_str)
                .is_some_and(|value| value == run_id)
        }),
        "continue should keep appending to the same run_id"
    );
    assert!(
        after_events.iter().skip(before_events.len()).any(|event| {
            event
                .get("payload")
                .and_then(|payload| payload.get("event_type"))
                .and_then(Value::as_str)
                == Some("run_started")
        }),
        "continued session should reopen the same run through the lifecycle handoff"
    );
    assert_eq!(session_run_dirs(&session_dir), vec![run_dir]);
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_operator_sidebar_primary() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;
    let session_dir = create_temp_session_dir();
    let run_id = "run_resume_diff";
    write_quiescent_resume_diff_fixture(&session_dir, run_id);
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_resumed_quiescent_session(
        PtyGeometry::PRIMARY_SIGNOFF,
        &session_dir,
        &config_path,
        run_id,
    );

    let _sidebar_screen = show_live_operator_sidebar(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        OPERATOR_FILES_MARKER,
    );
    let sidebar_visual = capture_manifest_backed_visual_checkpoint(
        "operator_sidebar",
        "operator_sidebar_primary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(OPERATOR_FILES_MARKER, 16, 6),
        &[
            "historical answer",
            OPERATOR_FILES_MARKER,
            "demo.txt",
            LIVE_READY_NEXT_TURN_MARKER,
        ],
    )
    .expect("capture operator sidebar primary image");

    assert!(visual_dir.join(&sidebar_visual.file_name).exists());
    assert!(sidebar_visual.manifest_json_path.exists());
    assert!(sidebar_visual.manifest_jsonl_path.exists());

    harness
        .child
        .kill()
        .expect("terminate operator sidebar harness");
    std::mem::forget(harness.child);
}

#[tokio::test(flavor = "current_thread")]
async fn pty_helper_operator_sidebar_session_contract() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let live_session_dir = create_temp_session_dir();
    let mut live = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &live_session_dir, |command| {
        command.arg("--mock");
    });
    wait_for_screen_contains(
        &mut live.parser,
        &live.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup home before opening live runtime contract shell");

    type_text(live.writer.as_mut(), "shell parity task");
    send_key(live.writer.as_mut(), b'\r').expect("submit mock prompt for live runtime contract");
    wait_for_screen_contains(
        &mut live.parser,
        &live.output_rx,
        LIVE_READY_NEXT_TURN_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for completed live runtime contract shell");

    let live_sidebar = show_live_operator_sidebar(
        &mut live.parser,
        &live.output_rx,
        live.writer.as_mut(),
        LIVE_OPERATOR_EMPTY_MARKER,
    );
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");
    let live_visual = capture_manifest_backed_visual_checkpoint(
        "operator_sidebar",
        "operator_sidebar_session_contract_live",
        &live.parser,
        &visual_dir,
        FocusCapture::anchored_exact(LIVE_OPERATOR_EMPTY_MARKER, 32, 8),
        &[
            "shell parity task",
            "Shell parity looks good.",
            LIVE_OPERATOR_EMPTY_MARKER,
            OPERATOR_FILES_MARKER,
            LIVE_READY_NEXT_TURN_MARKER,
        ],
    )
    .expect("capture live runtime contract image");
    assert!(live_sidebar.contains("shell parity task"));
    assert!(live_sidebar.contains("Shell parity looks good."));
    assert!(live_sidebar.contains(LIVE_OPERATOR_EMPTY_MARKER));
    assert!(live_sidebar.contains(OPERATOR_FILES_MARKER));
    assert!(live_sidebar.contains(LIVE_READY_NEXT_TURN_MARKER));
    assert!(visual_dir.join(&live_visual.file_name).exists());

    live.child
        .kill()
        .expect("terminate live runtime contract harness");
    std::mem::forget(live.child);

    let server = start_responses_wiremock_server().await;
    let continued_session_dir = create_temp_session_dir();
    let run_id = "run_resume_quiescent";
    write_quiescent_resume_fixture(&continued_session_dir, run_id);
    let config_path = write_wiremock_tui_config(&continued_session_dir, &server.uri());
    let mut continued = spawn_resumed_quiescent_session(
        PtyGeometry::PRIMARY_SIGNOFF,
        &continued_session_dir,
        &config_path,
        run_id,
    );

    let continued_sidebar = show_live_operator_sidebar(
        &mut continued.parser,
        &continued.output_rx,
        continued.writer.as_mut(),
        LIVE_OPERATOR_EMPTY_MARKER,
    );
    let continued_visual = capture_manifest_backed_visual_checkpoint(
        "operator_sidebar",
        "operator_sidebar_session_contract_continued",
        &continued.parser,
        &visual_dir,
        FocusCapture::anchored_exact(LIVE_OPERATOR_EMPTY_MARKER, 34, 8),
        &[
            "historical answer",
            LIVE_OPERATOR_EMPTY_MARKER,
            OPERATOR_FILES_MARKER,
            LIVE_READY_NEXT_TURN_MARKER,
        ],
    )
    .expect("capture continued runtime contract image");
    assert!(continued_sidebar.contains("Continued"));
    assert!(continued_sidebar.contains("historical answer"));
    assert!(continued_sidebar.contains(LIVE_OPERATOR_EMPTY_MARKER));
    assert!(continued_sidebar.contains(OPERATOR_FILES_MARKER));
    assert!(continued_sidebar.contains(LIVE_READY_NEXT_TURN_MARKER));
    assert!(visual_dir.join(&continued_visual.file_name).exists());

    compose_task_5_runtime_context_evidence(
        &visual_dir.join(&live_visual.file_name),
        &visual_dir.join(&continued_visual.file_name),
        &visual_dir.join("pty_operator_sidebar_session_contract.png"),
    )
    .expect("compose operator sidebar contract artifact");
    compose_task_5_runtime_context_evidence(
        &visual_dir.join(&live_visual.file_name),
        &visual_dir.join(&continued_visual.file_name),
        &task_5_runtime_context_evidence_path(),
    )
    .expect("compose task 5 runtime context evidence artifact");

    continued
        .child
        .kill()
        .expect("terminate continued runtime contract harness");
    std::mem::forget(continued.child);
}

#[test]
fn pty_e2e_sidebar_session_parity() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup home before opening sidebar parity session");

    type_text(harness.writer.as_mut(), "shell parity task");
    send_key(harness.writer.as_mut(), b'\r').expect("submit mock prompt for sidebar parity");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        LIVE_READY_NEXT_TURN_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for completed live session before opening sidebar parity");

    let sidebar_screen = show_live_operator_sidebar(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        LIVE_OPERATOR_EMPTY_MARKER,
    );
    let sidebar_visual = capture_manifest_backed_visual_checkpoint(
        "operator_sidebar",
        "sidebar_session_parity",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(LIVE_OPERATOR_EMPTY_MARKER, 18, 8),
        &[
            "shell parity task",
            "Shell parity looks good.",
            LIVE_OPERATOR_EMPTY_MARKER,
            OPERATOR_FILES_MARKER,
            LIVE_READY_NEXT_TURN_MARKER,
        ],
    )
    .expect("capture sidebar parity image");

    assert!(sidebar_screen.contains("shell parity task"));
    assert!(sidebar_screen.contains("Shell parity looks good."));
    assert!(sidebar_screen.contains(LIVE_OPERATOR_EMPTY_MARKER));
    assert!(sidebar_screen.contains(OPERATOR_FILES_MARKER));
    assert!(sidebar_screen.contains(LIVE_READY_NEXT_TURN_MARKER));
    assert!(!sidebar_screen.contains("Tabs"));
    assert!(visual_dir.join(&sidebar_visual.file_name).exists());
    assert!(sidebar_visual.manifest_json_path.exists());
    assert!(sidebar_visual.manifest_jsonl_path.exists());

    harness
        .child
        .kill()
        .expect("terminate sidebar parity harness");
    std::mem::forget(harness.child);
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_operator_sidebar_stays_usable_across_window_sizes() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    for (geometry, _snapshot_name, artifact_name) in [
        (
            PtyGeometry::HALF_SCREEN_SPLIT,
            "pty_operator_sidebar_split_tall",
            "operator_sidebar_split_tall",
        ),
        (
            PtyGeometry::SPLIT_TIER_WINDOW,
            "pty_operator_sidebar_split_tier",
            "operator_sidebar_split_tier",
        ),
    ] {
        let session_dir = create_temp_session_dir();
        let run_id = "run_resume_quiescent";
        write_quiescent_resume_fixture(&session_dir, run_id);
        let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
        let mut harness =
            spawn_resumed_quiescent_session(geometry, &session_dir, &config_path, run_id);

        let _details_screen = show_live_operator_sidebar(
            &mut harness.parser,
            &harness.output_rx,
            harness.writer.as_mut(),
            LIVE_OPERATOR_EMPTY_MARKER,
        );
        let details_visual = capture_visual_checkpoint(
            artifact_name,
            &harness.parser,
            &visual_dir,
            FocusCapture::anchored_exact(LIVE_OPERATOR_EMPTY_MARKER, 24, 6),
        )
        .expect("capture responsive operator sidebar image");

        assert!(visual_dir.join(&details_visual.file_name).exists());

        harness
            .child
            .kill()
            .expect("terminate responsive continued-session harness");
        std::mem::forget(harness.child);
    }
}

#[test]
fn pty_e2e_replay_transcript_parity_stays_visible_in_dense_layout() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let run_id = "run_native_tool_parity_dense";
    write_native_tool_parity_fixture(&session_dir, run_id);
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::SIX_WINDOW_DENSE, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_HOME_DENSE_VALUE_PROP_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher before dense parity replay");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(harness.writer.as_mut(), b'\r').expect("select dense parity replay session");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for dense parity replay shell");

    let current = harness.parser.screen().contents();
    let screen = stabilize_screen(&mut harness.parser, &harness.output_rx, current);
    let parity_visual = capture_manifest_backed_visual_checkpoint(
        "transcript_shell",
        "native_tool_parity_dense",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Inline transcript parity is easier to scan now.", 40, 5),
        &[
            "Inline transcript parity is easier to scan now.",
            REPLAY_DENSE_READY_MARKER,
        ],
    )
    .expect("capture dense replay transcript parity image");

    assert!(screen.contains("Inline transcript parity is easier to scan now."));
    assert!(!screen.contains("Event log"));
    assert!(!screen.contains("Keyboard Shortcuts:"));
    assert!(visual_dir.join(&parity_visual.file_name).exists());
    assert!(parity_visual.manifest_json_path.exists());
    assert!(parity_visual.manifest_jsonl_path.exists());

    harness
        .child
        .kill()
        .expect("terminate dense replay parity harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_child_session_navigation_checkpoint() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let parent_session_id = "000_parent_navigation";
    let child_session_id = "zzz_child_navigation";
    write_child_navigation_fixture(&session_dir, parent_session_id, child_session_id);
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher before child navigation replay");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    type_text(harness.writer.as_mut(), parent_session_id);
    send_key(harness.writer.as_mut(), b'\r').expect("select parent replay session");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        "Parent session can jump to child transcripts.",
        STARTUP_TIMEOUT,
    )
    .expect("wait for parent replay session before child navigation");

    send_key(harness.writer.as_mut(), 0x1d).expect("navigate to first child session");
    let child_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        child_session_id,
        MARKER_TIMEOUT,
    )
    .expect("wait for child replay session after navigation");
    let child_visual = capture_manifest_backed_visual_checkpoint(
        "replay_shell",
        "child_session_navigation",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(child_session_id, 24, 4),
        &[REPLAY_DENSE_READY_MARKER, child_session_id],
    )
    .expect("capture child-session navigation image");

    assert!(child_screen.contains(REPLAY_DENSE_READY_MARKER));
    assert!(child_screen.contains(child_session_id));
    assert!(visual_dir.join(&child_visual.file_name).exists());
    assert!(child_visual.manifest_json_path.exists());
    assert!(child_visual.manifest_jsonl_path.exists());

    send_key(harness.writer.as_mut(), 0x1b).expect("navigate back to parent session");
    let parent_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        "Parent session can jump to child transcripts.",
        MARKER_TIMEOUT,
    )
    .expect("wait for parent replay session after navigating back");
    assert!(parent_screen.contains("Parent session asks the child to inspect parity"));
    assert!(parent_screen.contains("Parent session can jump to child transcripts."));

    harness
        .child
        .kill()
        .expect("terminate child-session navigation harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_startup_launcher_stays_usable_in_split_and_dense_windows() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    for (geometry, _snapshot_name, artifact_name) in [
        (
            PtyGeometry::MINIMUM_SIGNOFF,
            "pty_startup_quarter_tile",
            "startup_quarter_tile",
        ),
        (
            PtyGeometry::HALF_SCREEN_SPLIT,
            "pty_startup_split_tall",
            "startup_split_tall",
        ),
        (
            PtyGeometry::SPLIT_TIER_WINDOW,
            "pty_startup_split_tier",
            "startup_split_tier",
        ),
        (
            PtyGeometry::SIX_WINDOW_DENSE,
            "pty_startup_six_window",
            "startup_six_window",
        ),
    ] {
        let mut harness = spawn_harness_tui(geometry, &session_dir, |command| {
            command.arg("--mock");
        });

        let ready_marker = if geometry == PtyGeometry::SIX_WINDOW_DENSE {
            STARTUP_HOME_DENSE_VALUE_PROP_MARKER
        } else {
            STARTUP_LAUNCHER_READY_MARKER
        };

        let _startup_screen = wait_for_screen_contains(
            &mut harness.parser,
            &harness.output_rx,
            ready_marker,
            STARTUP_TIMEOUT,
        )
        .expect("wait for startup launcher in alternate window shape");
        let startup_visual = capture_visual_checkpoint(
            artifact_name,
            &harness.parser,
            &visual_dir,
            FocusCapture::anchored_exact(ready_marker, 40, 2),
        )
        .expect("capture alternate startup launcher image");

        assert!(visual_dir.join(&startup_visual.file_name).exists());

        quit_startup_tui(
            &mut harness.parser,
            &harness.output_rx,
            harness.writer.as_mut(),
        );
        wait_for_process_exit(
            &mut harness.child,
            &harness.output_rx,
            &mut harness.parser,
            Duration::from_secs(10),
        )
        .expect("quit alternate startup launcher cleanly");
    }
}

#[test]
fn pty_e2e_continue_rejects_active_or_unrestorable_session() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    write_active_blocked_fixture(&session_dir, "run_active_blocked");
    write_corrupt_blocked_fixture(&session_dir, "run_unrestorable_blocked");
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher ready");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "resume",
        "tasks are still in flight",
    );
    send_key(harness.writer.as_mut(), b'\r').expect("attempt continue on active session");

    let active_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        "tasks are still in flight",
        MARKER_TIMEOUT,
    )
    .expect("wait for active-session rejection banner");
    let active_visual = capture_manifest_backed_visual_checkpoint(
        "continue_session",
        "continue_rejected_active",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("tasks are still in flight", 28, 1),
        &[STARTUP_CONTINUE_HISTORY_MARKER, "tasks are still in flight"],
    )
    .expect("capture active-session rejection image");
    insta::assert_snapshot!(
        "pty_continue_rejected_active",
        checkpoint_visual_snapshot_without_focus_position(
            &active_screen,
            &[STARTUP_CONTINUE_HISTORY_MARKER, "tasks are still in flight",],
            &active_visual,
        )
    );

    send_key(harness.writer.as_mut(), 0x1b).expect("close active-session rejection overlay");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for startup launcher after active-session rejection");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "resume",
        "events unavailable",
    );
    move_list_selection(harness.writer.as_mut(), 1);
    send_key(harness.writer.as_mut(), b'\r').expect("attempt continue on corrupt session");

    let unrestorable_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        "events unavailable",
        MARKER_TIMEOUT,
    )
    .expect("wait for unrestorable-session rejection banner");
    let unrestorable_visual = capture_manifest_backed_visual_checkpoint(
        "continue_session",
        "continue_rejected_unrestorable",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("events unavailable", 28, 1),
        &[STARTUP_CONTINUE_HISTORY_MARKER, "events unavailable"],
    )
    .expect("capture unrestorable-session rejection image");
    insta::assert_snapshot!(
        "pty_continue_rejected_unrestorable",
        checkpoint_visual_snapshot(
            &unrestorable_screen,
            &[STARTUP_CONTINUE_HISTORY_MARKER, "events unavailable",],
            &unrestorable_visual,
        )
    );

    send_key(harness.writer.as_mut(), 0x1b).expect("close unrestorable-session rejection overlay");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for startup launcher after unrestorable-session rejection");

    harness
        .child
        .kill()
        .expect("terminate blocked-session launcher after rejection coverage");
    std::mem::forget(harness.child);
}

#[test]
fn pty_helper_replay_read_only() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let run_dir = write_replay_fixture(&session_dir, "run_replay_safe");
    let events_path = run_dir.join("events.jsonl");
    let before =
        fs::read_to_string(&events_path).expect("read replay fixture events before launch");
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher ready");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(harness.writer.as_mut(), b'\r').expect("select replay session");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell to render");

    send_key(harness.writer.as_mut(), b'\t').expect("focus replay details pane");
    send_key(harness.writer.as_mut(), b'\t').expect("attempt replay prompt focus");
    type_text(harness.writer.as_mut(), "blocked in replay");
    send_key(harness.writer.as_mut(), b'\r').expect("attempt replay submit");

    let replay_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_READY_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for stable replay screen after submit attempt");
    assert!(replay_screen.contains(REPLAY_DENSE_READY_MARKER));
    assert!(replay_screen.contains("run_replay_safe"));
    assert!(replay_screen.contains("Replay is read-only"));
    assert!(!replay_screen.contains("Recent context for the next turn"));
    assert!(!replay_screen.contains("Enter send"));
    let replay_visual = capture_manifest_backed_visual_checkpoint(
        "replay",
        "replay_read_only",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(REPLAY_READY_MARKER, 20, 2),
        &[
            REPLAY_READY_MARKER,
            "run_replay_safe",
            REPLAY_DENSE_READY_MARKER,
            "Replay is read-only",
        ],
    )
    .expect("capture replay read-only image");
    insta::assert_snapshot!(
        "pty_replay_read_only",
        checkpoint_visual_snapshot(
            &replay_screen,
            &[
                REPLAY_READY_MARKER,
                "run_replay_safe",
                REPLAY_DENSE_READY_MARKER,
                "Replay is read-only"
            ],
            &replay_visual,
        )
    );

    harness.child.kill().expect("terminate replay PTY child");
    std::mem::forget(harness.child);

    let after = fs::read_to_string(&events_path).expect("read replay fixture events after launch");
    assert_eq!(after, before, "replay mode must not append to events.jsonl");
}

fn responses_api_sse_fixture() -> String {
    concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_123\"}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string()
}

#[test]
fn pty_signoff_helpers_cover_primary_and_minimum_geometries() {
    for (geometry, expected_cols, expected_rows) in [
        (PtyGeometry::MINIMUM_SIGNOFF, 80, 24),
        (PtyGeometry::HALF_SCREEN_SPLIT, 80, 48),
        (PtyGeometry::SPLIT_TIER_WINDOW, 96, 40),
        (PtyGeometry::SIX_WINDOW_DENSE, 60, 18),
        (
            PtyGeometry::PRIMARY_SIGNOFF,
            PRIMARY_SIGNOFF_COLS,
            PRIMARY_SIGNOFF_ROWS,
        ),
    ] {
        let size = geometry.pty_size();
        assert_eq!(size.cols, expected_cols);
        assert_eq!(size.rows, expected_rows);
        assert_eq!(
            geometry.parser().screen().size(),
            (expected_rows, expected_cols)
        );
    }

    let mut prompt_keys = Vec::new();
    LIVE_STATE_FIXTURES
        .prompt
        .focus_prompt(&mut prompt_keys)
        .expect("serialize prompt focus fixture");
    assert_eq!(prompt_keys, b"");

    let mut live_prompt_keys = Vec::new();
    focus_live_prompt(&mut live_prompt_keys).expect("serialize live prompt focus fixture");
    assert_eq!(live_prompt_keys, b"\t");

    let mut draft_bytes = Vec::new();
    LIVE_STATE_FIXTURES
        .draft
        .write_and_submit(&mut draft_bytes)
        .expect("serialize draft fixture");
    assert_eq!(draft_bytes, b"Hello from PTY\r");

    let mut preserved_draft_bytes = Vec::new();
    LIVE_STATE_FIXTURES
        .permission
        .write_preserved_draft(&mut preserved_draft_bytes)
        .expect("serialize preserved draft fixture");
    assert_eq!(preserved_draft_bytes, b"keep this draft");

    let mut permission_bytes = Vec::new();
    LIVE_STATE_FIXTURES
        .permission
        .prepare_stable_capture(&mut permission_bytes)
        .expect("serialize permission fixture");
    assert_eq!(permission_bytes.first().copied(), Some(0x19));
    assert_eq!(permission_bytes.get(1).copied(), Some(b' '));
    assert_eq!(
        permission_bytes.len(),
        2 + LIVE_STATE_FIXTURES.permission.rewind_steps
    );
}

#[test]
fn pty_visual_ttf_antialias_defaults_to_enabled() {
    assert!(parse_ttf_antialias_env(None));
    for value in ["1", "true", "on", " True ", " ON ", "unexpected"] {
        assert!(
            parse_ttf_antialias_env(Some(value)),
            "expected {value:?} to keep PTY TTF anti-aliasing enabled"
        );
    }
}

#[test]
fn pty_visual_ttf_antialias_honors_explicit_opt_out() {
    for value in ["0", "false", "off", " False ", " OFF "] {
        assert!(
            !parse_ttf_antialias_env(Some(value)),
            "expected {value:?} to disable PTY TTF anti-aliasing"
        );
    }
}

#[test]
fn focus_render_state_hash_tracks_cell_styles() {
    let mut plain = VtParser::new(1, 4, 0);
    plain.process(b"AB");

    let mut styled = VtParser::new(1, 4, 0);
    styled.process(b"\x1b[31;1mA\x1b[0mB");

    let plain_hash =
        blake3::hash(&extract_region_render_state(plain.screen(), (0, 0, 1, 2))).to_hex();
    let styled_hash =
        blake3::hash(&extract_region_render_state(styled.screen(), (0, 0, 1, 2))).to_hex();

    assert_ne!(plain_hash, styled_hash);
}

fn write_wiremock_tui_config(session_dir: &Path, wiremock_uri: &str) -> PathBuf {
    let config_path = session_dir.join("wiremock-tui-config.jsonc");
    let body = json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": format!("{wiremock_uri}/v1"),
                "api_key": "test-key",
                "api_mode": "responses",
                "models": {
                    "model-1": {
                        "display_name": "Model 1",
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "deep work agent",
                "system_prompt": "Deep prompt",
                "model_ref": "default:model-1",
                "tools": ["read"],
            },
            "worker": {
                "description": "worker agent",
                "system_prompt": "Worker prompt",
                "model_ref": "default:model-1",
                "tools": ["read"],
            }
        },
        "permissions": {
            "defaults": {
                "edit": "ask",
                "shell": "deny",
                "question": "ask",
                "task": "ask",
                "webfetch": "deny",
                "websearch": "deny",
                "codesearch": "deny",
                "lsp": "allow",
                "network": "deny",
            }
        },
        "runtime": {
            "background_tasks": {
                "defaultConcurrency": 2,
                "providerConcurrency": 2,
                "modelConcurrency": 2,
                "staleTimeoutMs": 15000,
                "messageStalenessTimeoutMs": 5000,
            },
            "session_dir": session_dir.display().to_string(),
            "deterministic": {
                "enabled": true,
                "seed": 42,
            }
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp",
                "auth_token": Value::Null,
                "require_auth": false,
                "timeout_secs": 30,
                "max_retries": 1,
                "retry_backoff_ms": 250,
            }
        },
        "ui": {
            "default_profile": "worker",
        }
    });
    fs::write(
        &config_path,
        serde_json::to_string_pretty(&body).expect("serialize temporary wiremock TUI config"),
    )
    .expect("write temporary wiremock TUI config");
    config_path
}

fn wait_for_screen_contains(
    parser: &mut vt100::Parser,
    output_rx: &Receiver<Vec<u8>>,
    needle: &str,
    timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;

    loop {
        drain_output(parser, output_rx);

        let current = screen_contents(parser);
        if current.contains(needle) {
            return Ok(stabilize_screen(parser, output_rx, current));
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "timed out waiting for screen marker '{needle}' after {timeout:?}; final screen:\n{current}"
            ));
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!(
                    "pty output stream closed while waiting for '{needle}'; last screen:\n{current}"
                ));
            }
        }
    }
}

fn focus_live_prompt(writer: &mut dyn Write) -> std::io::Result<()> {
    send_key(writer, b'\t')
}

#[test]
fn pty_visual_regression_contract_covers_redesigned_surface_families() {
    let mut unique_pngs = std::collections::BTreeSet::new();
    let mut unique_snapshots = std::collections::BTreeSet::new();
    let mut covered_family_states = std::collections::BTreeSet::new();

    for contract in OFFLINE_VISUAL_EVIDENCE_CONTRACTS {
        assert!(
            contract.png.starts_with("pty_") && contract.png.ends_with(".png"),
            "offline evidence PNG must stay under the pty_* naming contract: {contract:?}"
        );
        assert!(
            unique_pngs.insert(contract.png),
            "offline evidence PNG names must stay unique: {}",
            contract.png
        );
        if let Some(snapshot) = contract.snapshot {
            assert!(
                snapshot.starts_with("pty_") || snapshot.starts_with("native_tool_parity_"),
                "offline evidence snapshot must stay under the pty_* naming contract: {contract:?}"
            );
            assert!(
                unique_snapshots.insert(snapshot),
                "offline evidence snapshot names must stay unique: {}",
                snapshot
            );
        }
        covered_family_states.insert((contract.family, contract.state));
    }

    for required in [
        ("startup_shell", "happy_path"),
        ("startup_command_palette", "happy_path"),
        ("startup_session_history", "continue_history"),
        ("startup_session_history", "replay_history"),
        ("continue_session", "happy_path"),
        ("continue_session", "failure_path"),
        ("permission", "happy_path"),
        ("live_shell", "happy_path"),
        ("live_shell", "inline_completion"),
        ("replay_shell", "happy_path"),
        ("replay_shell", "child_navigation"),
        ("transcript_shell", "happy_path"),
        ("transcript_shell", "dense_parity"),
        ("operator_sidebar", "happy_path"),
        ("operator_sidebar", "parity"),
        ("replay", "failure_path"),
    ] {
        assert!(
            covered_family_states.contains(&required),
            "offline PTY corpus must cover {:?}",
            required
        );
    }
}

#[test]
fn pty_e2e_snapshots_are_stable() {
    let snapshot_dir = repo_root()
        .join("crates")
        .join("harness-testkit")
        .join("tests")
        .join("snapshots");
    let mut expected = OFFLINE_VISUAL_EVIDENCE_CONTRACTS
        .iter()
        .filter_map(|contract| contract.snapshot)
        .map(|snapshot| format!("pty_e2e__{snapshot}.snap"))
        .collect::<std::collections::BTreeSet<_>>();
    expected.extend(
        [
            "pty_e2e__native_tool_parity_thinking.snap",
            "pty_e2e__native_tool_parity_dense.snap",
            "pty_e2e__pty_operator_sidebar_primary.snap",
            "pty_e2e__pty_session_shell_primary_live.snap",
        ]
        .into_iter()
        .map(str::to_string),
    );
    let actual = fs::read_dir(&snapshot_dir)
        .expect("read harness-testkit snapshot dir")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            (path.extension().and_then(|ext| ext.to_str()) == Some("snap")).then_some(name)
        })
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(actual, expected, "offline PTY snapshot set drifted");
}

#[test]
fn checkpoint_snapshot_manifest_paths_drop_checkout_root() {
    assert_eq!(
        stable_manifest_snapshot_path(Path::new(
            "/tmp/repo/target/pty-visual-artifacts/pty-manifests/live_shell/manifest.json",
        )),
        "pty-manifests/live_shell/manifest.json"
    );
    assert_eq!(
        stable_manifest_snapshot_path(Path::new(
            "/tmp/repo/target/pty-visual-artifacts/pty-manifests/live_shell/manifest.jsonl",
        )),
        "pty-manifests/live_shell/manifest.jsonl"
    );
}

fn stabilize_screen(
    parser: &mut vt100::Parser,
    output_rx: &Receiver<Vec<u8>>,
    initial: String,
) -> String {
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

        let current = screen_contents(parser);
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

fn stabilize_focus_render_state(
    parser: &mut vt100::Parser,
    output_rx: &Receiver<Vec<u8>>,
    focus: FocusCapture,
) -> String {
    let mut latest_screen = screen_contents(parser);
    let mut latest_render_hash = focus_render_state_blake3(parser.screen(), focus);
    let mut stable_since = Instant::now();
    let deadline = Instant::now() + STABLE_TIMEOUT;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return latest_screen;
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        match output_rx.recv_timeout(wait_timeout) {
            Ok(chunk) => parser.process(&chunk),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return latest_screen,
        }

        let current_screen = screen_contents(parser);
        let current_render_hash = focus_render_state_blake3(parser.screen(), focus);
        if current_screen != latest_screen || current_render_hash != latest_render_hash {
            latest_screen = current_screen;
            latest_render_hash = current_render_hash;
            stable_since = Instant::now();
            continue;
        }

        if Instant::now().saturating_duration_since(stable_since) >= STABLE_WINDOW {
            return latest_screen;
        }
    }
}

fn focus_render_state_blake3(screen: &vt100::Screen, focus: FocusCapture) -> String {
    let (rows, cols) = screen.size();
    let focus_region = find_marker_cell(screen, focus.marker)
        .map(|(row, col)| anchored_region((row, col), (rows, cols), focus))
        .unwrap_or((0, 0, rows.max(1), cols.max(1)));
    blake3::hash(&extract_region_render_state(screen, focus_region))
        .to_hex()
        .to_string()
}

fn drain_output(parser: &mut vt100::Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn screen_contents(parser: &vt100::Parser) -> String {
    parser.screen().contents()
}

#[derive(Debug)]
struct VisualCheckpoint {
    file_name: String,
    manifest_json_path: PathBuf,
    manifest_jsonl_path: PathBuf,
    focus_marker_found: bool,
    focus_pixels_blake3: String,
    focus_render_state_blake3: String,
    focus_marker: String,
    focus_region_cells: (u16, u16, u16, u16),
    size_px: (u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FocusCapture {
    marker: &'static str,
    width_cells: u16,
    height_cells: u16,
    top_padding_cells: u16,
    left_padding_cells: u16,
}

impl FocusCapture {
    const fn anchored(marker: &'static str, width_cells: u16, height_cells: u16) -> Self {
        Self {
            marker,
            width_cells,
            height_cells,
            top_padding_cells: 2,
            left_padding_cells: 2,
        }
    }

    const fn anchored_exact(marker: &'static str, width_cells: u16, height_cells: u16) -> Self {
        Self {
            marker,
            width_cells,
            height_cells,
            top_padding_cells: 0,
            left_padding_cells: 0,
        }
    }
}

fn checkpoint_visual_snapshot(screen: &str, markers: &[&str], visual: &VisualCheckpoint) -> String {
    let mut lines = marker_presence_lines(screen, markers);
    lines.push(format!("focus_marker: {}", visual.focus_marker));
    // Snapshot terminal render state here so one-cell raster drift does not
    // destabilize the deterministic PTY contract.
    lines.push(format!(
        "focus_render_state_blake3: {}",
        visual.focus_render_state_blake3
    ));
    lines.push(format!(
        "focus_region_cells: row={}, col={}, height={}, width={}",
        visual.focus_region_cells.0,
        visual.focus_region_cells.1,
        visual.focus_region_cells.2,
        visual.focus_region_cells.3
    ));
    lines.push(format!("image: {}", visual.file_name));
    lines.push(format!(
        "manifest_json: {}",
        stable_manifest_snapshot_path(&visual.manifest_json_path)
    ));
    lines.push(format!(
        "manifest_jsonl: {}",
        stable_manifest_snapshot_path(&visual.manifest_jsonl_path)
    ));
    lines.push(format!(
        "size_px: {}x{}",
        visual.size_px.0, visual.size_px.1
    ));
    lines.join("\n")
}

fn checkpoint_visual_snapshot_without_focus_hash(
    screen: &str,
    markers: &[&str],
    visual: &VisualCheckpoint,
) -> String {
    checkpoint_visual_snapshot(screen, markers, visual)
        .lines()
        .filter(|line| !line.starts_with("focus_render_state_blake3:"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn checkpoint_visual_snapshot_without_focus_position(
    screen: &str,
    markers: &[&str],
    visual: &VisualCheckpoint,
) -> String {
    checkpoint_visual_snapshot(screen, markers, visual)
        .lines()
        .filter(|line| {
            !line.starts_with("focus_render_state_blake3:")
                && !line.starts_with("focus_region_cells:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stable_manifest_snapshot_path(path: &Path) -> String {
    let components = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    components
        .iter()
        .position(|component| component == "pty-manifests")
        .map(|index| components[index..].join("/"))
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

fn family_manifest_dir(visual_dir: &Path, family: &str) -> PathBuf {
    visual_dir.join("pty-manifests").join(family)
}

fn marker_presence_states(screen: &str, markers: &[&str]) -> Vec<(String, bool)> {
    markers
        .iter()
        .map(|marker| ((*marker).to_string(), screen.contains(marker)))
        .collect()
}

fn write_family_manifest_entry(
    family: &str,
    checkpoint: &VisualCheckpoint,
    checkpoint_id: &str,
    screen_markers: &[(String, bool)],
) -> Result<(), String> {
    let manifest_entry = json!({
        "checkpoint_id": checkpoint_id,
        "png_path": checkpoint.file_name,
        "screen_markers": screen_markers.iter().map(|(marker, present)| {
            json!({
                "marker": marker,
                "present": present,
            })
        }).collect::<Vec<_>>(),
        "captured_at_stage": checkpoint_id,
        "focus": {
            "marker": checkpoint.focus_marker,
            "found": checkpoint.focus_marker_found,
            "scope": if checkpoint.focus_marker_found {
                "anchored"
            } else {
                "full_frame_fallback"
            },
            "pixels_blake3": checkpoint.focus_pixels_blake3,
            "render_state_blake3": checkpoint.focus_render_state_blake3,
        },
        "region": {
            "row": checkpoint.focus_region_cells.0,
            "col": checkpoint.focus_region_cells.1,
            "height": checkpoint.focus_region_cells.2,
            "width": checkpoint.focus_region_cells.3,
        },
        "image": {
            "file_name": checkpoint.file_name,
            "size_px": {
                "width": checkpoint.size_px.0,
                "height": checkpoint.size_px.1,
            }
        },
        "metadata": {
            "family": family,
        },
    });

    let existing_entries = if checkpoint.manifest_json_path.exists() {
        let body = fs::read_to_string(&checkpoint.manifest_json_path).map_err(|err| {
            format!(
                "failed to read PTY family manifest {}: {err}",
                checkpoint.manifest_json_path.display()
            )
        })?;
        serde_json::from_str::<Value>(&body)
            .map_err(|err| format!("failed to parse PTY family manifest JSON: {err}"))?
            .get("checkpoints")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut checkpoints = existing_entries
        .into_iter()
        .filter(|entry| {
            entry
                .get("checkpoint_id")
                .and_then(Value::as_str)
                .is_none_or(|existing| existing != checkpoint_id)
        })
        .collect::<Vec<_>>();
    checkpoints.push(manifest_entry.clone());
    checkpoints.sort_by_key(|entry| {
        entry
            .get("checkpoint_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    });

    let manifest = json!({
        "family": family,
        "checkpoints": checkpoints,
    });
    let manifest_body = serde_json::to_string_pretty(&manifest)
        .map_err(|err| format!("failed to serialize PTY family manifest JSON: {err}"))?;
    fs::write(&checkpoint.manifest_json_path, manifest_body).map_err(|err| {
        format!(
            "failed to write PTY family manifest {}: {err}",
            checkpoint.manifest_json_path.display()
        )
    })?;

    let jsonl = checkpoints
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        &checkpoint.manifest_jsonl_path,
        if jsonl.is_empty() {
            jsonl
        } else {
            format!("{jsonl}\n")
        },
    )
    .map_err(|err| {
        format!(
            "failed to write PTY family manifest {}: {err}",
            checkpoint.manifest_jsonl_path.display()
        )
    })
}

fn capture_manifest_backed_visual_checkpoint(
    family: &str,
    checkpoint_id: &str,
    parser: &VtParser,
    visual_dir: &Path,
    focus: FocusCapture,
    markers: &[&str],
) -> Result<VisualCheckpoint, String> {
    let mut checkpoint = capture_visual_checkpoint(checkpoint_id, parser, visual_dir, focus)?;
    let manifest_dir = family_manifest_dir(visual_dir, family);
    fs::create_dir_all(&manifest_dir).map_err(|err| {
        format!(
            "failed to create PTY family manifest dir {}: {err}",
            manifest_dir.display()
        )
    })?;
    checkpoint.manifest_json_path = manifest_dir.join(VISUAL_MANIFEST_JSON_FILE);
    checkpoint.manifest_jsonl_path = manifest_dir.join(VISUAL_MANIFEST_JSONL_FILE);
    write_family_manifest_entry(
        family,
        &checkpoint,
        checkpoint_id,
        &marker_presence_states(&screen_contents(parser), markers),
    )?;
    Ok(checkpoint)
}

fn marker_presence_lines(screen: &str, markers: &[&str]) -> Vec<String> {
    markers
        .iter()
        .map(|marker| format!("{marker}: {}", screen.contains(marker)))
        .collect()
}

fn capture_visual_checkpoint(
    name: &str,
    parser: &VtParser,
    visual_dir: &Path,
    focus: FocusCapture,
) -> Result<VisualCheckpoint, String> {
    let image = render_parser_to_image(parser, PTY_RENDER_CONFIG);
    let file_name = format!("pty_{name}.png");
    let path = visual_dir.join(&file_name);
    image
        .save(&path)
        .map_err(|err| format!("failed to save {}: {err}", path.display()))?;

    let (rows, cols) = parser.screen().size();
    let focus_region = find_marker_cell(parser.screen(), focus.marker)
        .map(|(row, col)| anchored_region((row, col), (rows, cols), focus))
        .unwrap_or((0, 0, rows.max(1), cols.max(1)));
    let focus_marker_found = find_marker_cell(parser.screen(), focus.marker).is_some();

    let focus_pixels = extract_region_pixels(&image, focus_region, PTY_RENDER_CONFIG);
    let focus_render_state = extract_region_render_state(parser.screen(), focus_region);

    Ok(VisualCheckpoint {
        file_name,
        manifest_json_path: PathBuf::new(),
        manifest_jsonl_path: PathBuf::new(),
        focus_marker_found,
        focus_pixels_blake3: blake3::hash(&focus_pixels).to_hex().to_string(),
        focus_render_state_blake3: blake3::hash(&focus_render_state).to_hex().to_string(),
        focus_marker: focus.marker.to_string(),
        focus_region_cells: focus_region,
        size_px: (image.width(), image.height()),
    })
}

fn task_5_runtime_context_evidence_path() -> PathBuf {
    repo_root()
        .join(".sisyphus")
        .join("evidence")
        .join("task-5-live-runtime-context.png")
}

fn compose_task_5_runtime_context_evidence(
    live_path: &Path,
    continued_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let live = image::open(live_path)
        .map_err(|err| {
            format!(
                "failed to open live runtime artifact {}: {err}",
                live_path.display()
            )
        })?
        .to_rgb8();
    let continued = image::open(continued_path)
        .map_err(|err| {
            format!(
                "failed to open continued runtime artifact {}: {err}",
                continued_path.display()
            )
        })?
        .to_rgb8();

    let gap = 16u32;
    let width = live
        .width()
        .saturating_add(gap)
        .saturating_add(continued.width());
    let height = live.height().max(continued.height());
    let mut canvas = RgbImage::from_pixel(width, height, Rgb([0, 0, 0]));
    canvas
        .copy_from(&live, 0, 0)
        .map_err(|err| format!("failed to place live runtime image: {err}"))?;
    canvas
        .copy_from(&continued, live.width().saturating_add(gap), 0)
        .map_err(|err| format!("failed to place continued runtime image: {err}"))?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create evidence dir {}: {err}", parent.display()))?;
    }
    canvas.save(output_path).map_err(|err| {
        format!(
            "failed to save composed runtime evidence {}: {err}",
            output_path.display()
        )
    })
}

fn find_marker_cell(screen: &vt100::Screen, marker: &str) -> Option<(u16, u16)> {
    let (rows, cols) = screen.size();

    for row in 0..rows {
        let text = (0..cols)
            .filter_map(|col| screen.cell(row, col))
            .map(|cell| {
                let glyph = cell.contents();
                if glyph.is_empty() {
                    " ".to_string()
                } else {
                    glyph
                }
            })
            .collect::<String>();

        if let Some(byte_idx) = text.find(marker) {
            let col = text[..byte_idx].chars().count();
            if let Ok(col) = u16::try_from(col) {
                return Some((row, col));
            }
        }
    }

    None
}

fn anchored_region(
    anchor: (u16, u16),
    bounds: (u16, u16),
    focus: FocusCapture,
) -> (u16, u16, u16, u16) {
    let (anchor_row, anchor_col) = anchor;
    let (rows, cols) = bounds;
    let row_start = anchor_row.saturating_sub(focus.top_padding_cells);
    let col_start = anchor_col.saturating_sub(focus.left_padding_cells);

    let max_height = rows.saturating_sub(row_start).max(1);
    let max_width = cols.saturating_sub(col_start).max(1);

    let height = focus.height_cells.min(max_height).max(1);
    let width = focus.width_cells.min(max_width).max(1);

    (row_start, col_start, height, width)
}

fn visual_artifacts_dir() -> PathBuf {
    static PREPARED: OnceLock<()> = OnceLock::new();
    let explicit_dir = std::env::var("HARNESS_VISUAL_ARTIFACT_DIR").ok();
    let dir = explicit_dir
        .as_ref()
        .map(resolve_artifact_root)
        .unwrap_or_else(|| repo_root().join("target").join("pty-visual-artifacts"));
    let should_prune = explicit_dir.is_none();

    PREPARED.get_or_init(|| {
        fs::create_dir_all(&dir).expect("create visual artifacts dir");
        if should_prune {
            prune_stale_pty_visual_artifacts(&dir).expect("prune stale PTY visual artifacts");
        }
    });

    dir
}

fn resolve_artifact_root(dir: impl Into<PathBuf>) -> PathBuf {
    let dir = dir.into();
    if dir.is_absolute() {
        dir
    } else {
        repo_root().join(dir)
    }
}

fn prune_stale_pty_visual_artifacts(visual_dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(visual_dir).map_err(|err| {
        format!(
            "failed to read PTY visual artifacts dir {}: {err}",
            visual_dir.display()
        )
    })?;

    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "pty-manifests")
        {
            fs::remove_dir_all(&path).map_err(|err| {
                format!(
                    "failed to remove stale PTY manifest dir {}: {err}",
                    path.display()
                )
            })?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("pty_") || !file_name.ends_with(".png") {
            continue;
        }
        fs::remove_file(&path).map_err(|err| {
            format!(
                "failed to remove stale PTY artifact {}: {err}",
                path.display()
            )
        })?;
    }

    Ok(())
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

fn send_key(writer: &mut dyn Write, key: u8) -> std::io::Result<()> {
    writer.write_all(&[key])?;
    writer.flush()
}

fn resolve_harness_bin() -> PathBuf {
    if let Ok(path) = std::env::var("HARNESS_BIN") {
        let harness_bin = PathBuf::from(path);
        assert!(
            harness_bin.exists(),
            "HARNESS_BIN points to missing path: {}",
            harness_bin.display()
        );
        return harness_bin;
    }

    let repo = repo_root();
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("harness")
        .current_dir(&repo)
        .status()
        .expect("spawn cargo build -p harness");
    assert!(
        status.success(),
        "cargo build -p harness failed with status {status}"
    );

    let harness_bin = repo
        .join("target")
        .join("debug")
        .join(binary_name("harness"));
    assert!(
        harness_bin.exists(),
        "expected harness binary at {}",
        harness_bin.display()
    );
    harness_bin
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn create_temp_session_dir() -> PathBuf {
    static TEMP_SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

    let base = std::env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp session dir");

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let sequence = TEMP_SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = base.join(format!(
        "pty-e2e-{}-{suffix}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create unique temp session dir");
    dir
}

fn session_run_dirs(session_dir: &Path) -> Vec<PathBuf> {
    let mut run_dirs = fs::read_dir(session_dir)
        .expect("read session dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir() && path.join("events.jsonl").exists())
        .collect::<Vec<_>>();
    run_dirs.sort();
    run_dirs
}

fn load_events_jsonl(events_path: &Path) -> Vec<Value> {
    fs::read_to_string(events_path)
        .expect("read events jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse events jsonl line"))
        .collect()
}

fn write_quiescent_resume_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
    write_quiescent_resume_fixture_with_optional_diff(session_dir, run_id, false)
}

fn write_quiescent_resume_diff_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
    write_quiescent_resume_fixture_with_optional_diff(session_dir, run_id, true)
}

fn write_quiescent_resume_fixture_with_optional_diff(
    session_dir: &Path,
    run_id: &str,
    with_diff_artifact: bool,
) -> PathBuf {
    let mut events = vec![
        session_event(
            run_id,
            1,
            event_actor("system", Some("coordinator")),
            None,
            "run_started",
            json!({
                "run_name": "interactive",
                "workspace_root": "/workspace/project",
            }),
        ),
        session_event(
            run_id,
            2,
            event_actor("system", Some("coordinator")),
            None,
            "agent_spawned",
            json!({
                "agent_id": "agent_000001",
                "profile": "worker",
                "parent_agent_id": Value::Null,
            }),
        ),
        session_event(
            run_id,
            3,
            event_actor("user", Some("interactive-user")),
            Some("req_000001"),
            "user_message_submitted",
            json!({
                "request_id": "req_000001",
                "text": "historical question",
            }),
        ),
        session_event(
            run_id,
            4,
            event_actor("worker", Some("agent_000001")),
            Some("req_000001"),
            "provider_request_started",
            json!({
                "request_id": "req_000001",
                "provider_id": "default",
                "model_id": "model-1",
                "prompt_summary": "historical question",
                "request_digest": "digest-historical-request",
            }),
        ),
        session_event(
            run_id,
            5,
            event_actor("worker", Some("agent_000001")),
            Some("req_000001"),
            "provider_stream_delta",
            json!({
                "request_id": "req_000001",
                "delta": "historical answer",
            }),
        ),
        session_event(
            run_id,
            6,
            event_actor("worker", Some("agent_000001")),
            Some("req_000001"),
            "task_completed",
            json!({
                "task_id": "task_000001",
                "result_summary": "historical answer",
                "result_digest": "digest-historical-answer",
            }),
        ),
    ];

    if with_diff_artifact {
        events.push(session_event(
            run_id,
            7,
            event_actor("worker", Some("agent_000001")),
            Some("req_000001"),
            "edit_applied",
            json!({
                "edit_id": "edit_resume_diff",
                "path": "demo.txt",
                "new_file_digest": "digest-resume-diff-file",
                "diff_rel_path": "artifacts/edit-resume-diff.diff",
                "diff_digest": "digest-resume-diff-artifact",
            }),
        ));
    }

    events.push(session_event(
        run_id,
        if with_diff_artifact { 8 } else { 7 },
        event_actor("system", Some("coordinator")),
        None,
        "run_finished",
        json!({
            "summary": "segment complete",
        }),
    ));

    let run_dir = write_session_fixture(session_dir, run_id, &events);
    if with_diff_artifact {
        write_diff_artifact(
            &run_dir,
            "artifacts/edit-resume-diff.diff",
            sample_diff_artifact(),
        );
    }
    run_dir
}

fn write_child_navigation_fixture(
    session_dir: &Path,
    parent_session_id: &str,
    child_session_id: &str,
) {
    write_session_fixture(
        session_dir,
        child_session_id,
        &[
            session_event(
                child_session_id,
                1,
                event_actor("system", Some("coordinator")),
                None,
                "run_started",
                json!({
                    "run_name": "child replay",
                    "workspace_root": "/workspace/project",
                }),
            ),
            session_event(
                child_session_id,
                2,
                event_actor("system", Some("coordinator")),
                None,
                "agent_spawned",
                json!({
                    "agent_id": child_session_id,
                    "profile": "worker",
                    "parent_agent_id": "agent_000001",
                }),
            ),
            session_event(
                child_session_id,
                3,
                event_actor("user", Some(child_session_id)),
                Some("req_child_navigation"),
                "user_message_submitted",
                json!({
                    "request_id": "req_child_navigation",
                    "text": "Child session replay proof",
                }),
            ),
            session_event(
                child_session_id,
                4,
                event_actor("worker", Some(child_session_id)),
                Some("req_child_navigation"),
                "provider_stream_delta",
                json!({
                    "request_id": "req_child_navigation",
                    "delta": "Child session captured sidebar parity.",
                }),
            ),
            session_event(
                child_session_id,
                5,
                event_actor("system", Some("coordinator")),
                Some("req_child_navigation"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_child_lsp",
                    "tool_id": "code.lsp",
                    "args_summary": "{\"path\":\"src/app.rs\",\"kind\":\"diagnostics\"}",
                    "args_digest": "digest-child-lsp-args",
                    "metadata": {
                        "canonical_tool_id": "code.lsp",
                        "lineage": {
                            "parent_session_id": parent_session_id,
                            "parent_request_id": "req_parent_navigation",
                            "child_session_id": child_session_id,
                            "child_request_id": "req_child_navigation",
                        }
                    }
                }),
            ),
            session_event(
                child_session_id,
                6,
                event_actor("system", Some("coordinator")),
                Some("req_child_navigation"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_child_lsp",
                    "status": "succeeded",
                    "output_summary": "LSP findings stayed in the operator rail.",
                    "output_digest": "digest-child-lsp-output",
                    "output_json": Value::Null,
                    "metadata": {
                        "canonical_tool_id": "code.lsp",
                        "lineage": {
                            "parent_session_id": parent_session_id,
                            "parent_request_id": "req_parent_navigation",
                            "child_session_id": child_session_id,
                            "child_request_id": "req_child_navigation",
                        },
                        "timing": {
                            "elapsed_ms": 1100,
                        }
                    }
                }),
            ),
            session_event(
                child_session_id,
                7,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "child navigation captured",
                }),
            ),
        ],
    );

    write_session_fixture(
        session_dir,
        parent_session_id,
        &[
            session_event(
                parent_session_id,
                1,
                event_actor("system", Some("coordinator")),
                None,
                "run_started",
                json!({
                    "run_name": "interactive",
                    "workspace_root": "/workspace/project",
                }),
            ),
            session_event(
                parent_session_id,
                2,
                event_actor("system", Some("coordinator")),
                None,
                "agent_spawned",
                json!({
                    "agent_id": "agent_000001",
                    "profile": "worker",
                    "parent_agent_id": Value::Null,
                }),
            ),
            session_event(
                parent_session_id,
                3,
                event_actor("user", Some("interactive-user")),
                Some("req_parent_navigation"),
                "user_message_submitted",
                json!({
                    "request_id": "req_parent_navigation",
                    "text": "Parent session asks the child to inspect parity",
                }),
            ),
            session_event(
                parent_session_id,
                4,
                event_actor("system", Some("coordinator")),
                Some("req_parent_navigation"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_parent_spawn",
                    "tool_id": "agent.spawn",
                    "args_summary": format!(
                        "{{\"description\":\"Inspect parity child session\",\"subagent_type\":\"worker\",\"child_session_id\":\"{child_session_id}\"}}"
                    ),
                    "args_digest": "digest-parent-spawn-args",
                    "metadata": {
                        "canonical_tool_id": "agent.spawn",
                        "lineage": {
                            "parent_request_id": "req_parent_navigation",
                            "child_session_id": child_session_id,
                            "child_request_id": "req_child_navigation",
                        }
                    }
                }),
            ),
            session_event(
                parent_session_id,
                5,
                event_actor("system", Some("coordinator")),
                Some("req_parent_navigation"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_parent_spawn",
                    "status": "succeeded",
                    "output_summary": format!("task_id: {child_session_id}\nrequest_id: req_child_navigation"),
                    "output_digest": "digest-parent-spawn-output",
                    "output_json": {
                        "description": "Inspect parity child session",
                        "profile": "worker",
                        "mode": "foreground",
                        "status": "completed",
                        "duration_ms": 1100,
                        "result_summary": "Child session captured sidebar parity.",
                        "child_tool_call_count": 1,
                        "child_session_id": child_session_id,
                        "child_request_id": "req_child_navigation",
                    },
                    "metadata": {
                        "canonical_tool_id": "agent.spawn",
                        "lineage": {
                            "parent_request_id": "req_parent_navigation",
                            "child_session_id": child_session_id,
                            "child_request_id": "req_child_navigation",
                        },
                        "timing": {
                            "elapsed_ms": 1100,
                        }
                    }
                }),
            ),
            session_event(
                parent_session_id,
                6,
                event_actor("worker", Some("agent_000001")),
                Some("req_parent_navigation"),
                "provider_stream_delta",
                json!({
                    "request_id": "req_parent_navigation",
                    "delta": "Parent session can jump to child transcripts.",
                }),
            ),
            session_event(
                parent_session_id,
                7,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "parent navigation captured",
                }),
            ),
        ],
    );
}

fn write_native_tool_parity_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
    write_session_fixture(
        session_dir,
        run_id,
        &[
            session_event_with_ts(
                run_id,
                1,
                Some("2026-03-22T14:35:00Z"),
                event_actor("system", Some("coordinator")),
                None,
                "run_started",
                json!({
                    "run_name": "interactive",
                    "workspace_root": "/workspace/project",
                }),
            ),
            session_event_with_ts(
                run_id,
                2,
                Some("2026-03-22T14:35:01Z"),
                event_actor("system", Some("coordinator")),
                None,
                "agent_spawned",
                json!({
                    "agent_id": "agent_000001",
                    "profile": "worker",
                    "parent_agent_id": Value::Null,
                }),
            ),
            session_event_with_ts(
                run_id,
                3,
                Some("2026-03-22T14:35:12Z"),
                event_actor("user", Some("interactive-user")),
                Some("req_native_tool_parity"),
                "user_message_submitted",
                json!({
                    "request_id": "req_native_tool_parity",
                    "text": "Bring native tool parity inline",
                }),
            ),
            session_event_with_ts(
                run_id,
                4,
                Some("2026-03-22T14:35:13Z"),
                event_actor("worker", Some("agent_000001")),
                Some("req_native_tool_parity"),
                "provider_request_started",
                json!({
                    "request_id": "req_native_tool_parity",
                    "provider_id": "default",
                    "model_id": "model-1",
                    "prompt_summary": "Bring native tool parity inline",
                    "request_digest": "digest-native-tool-parity-request",
                }),
            ),
            session_event_with_ts(
                run_id,
                5,
                Some("2026-03-22T14:35:14Z"),
                event_actor("worker", Some("agent_000001")),
                Some("req_native_tool_parity"),
                "provider_reasoning_delta",
                json!({
                    "request_id": "req_native_tool_parity",
                    "delta": "Drafting the inline parity pass.",
                }),
            ),
            session_event_with_ts(
                run_id,
                6,
                Some("2026-03-22T14:35:20Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_native_read",
                    "tool_id": "read",
                    "args_summary": "{\"path\":\"src/ui.rs\",\"offset\":12,\"limit\":24}",
                    "args_digest": "digest-native-read-args",
                    "metadata": {
                        "canonical_tool_id": "read",
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                7,
                Some("2026-03-22T14:35:21Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_native_read",
                    "status": "succeeded",
                    "output_summary": "24 lines read from src/ui.rs",
                    "output_digest": "digest-native-read-output",
                    "output_json": Value::Null,
                    "metadata": {
                        "canonical_tool_id": "read",
                        "timing": {
                            "elapsed_ms": 1250,
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                8,
                Some("2026-03-22T14:35:27Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_alias_read",
                    "tool_id": "read",
                    "args_summary": "{\"path\":\"src/ui.rs\",\"offset\":12,\"limit\":24}",
                    "args_digest": "digest-alias-read-args",
                    "metadata": {
                        "canonical_tool_id": "read",
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                9,
                Some("2026-03-22T14:35:28Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_alias_read",
                    "status": "succeeded",
                    "output_summary": "24 lines read from src/ui.rs",
                    "output_digest": "digest-alias-read-output",
                    "output_json": Value::Null,
                    "metadata": {
                        "canonical_tool_id": "read",
                        "timing": {
                            "elapsed_ms": 1250,
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                10,
                Some("2026-03-22T14:36:01Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_task_alias",
                    "tool_id": "task",
                    "args_summary": "{\"description\":\"audit transcript parity\",\"subagent_type\":\"researcher\"}",
                    "args_digest": "digest-task-alias-args",
                    "metadata": {
                        "canonical_tool_id": "task",
                        "lineage": {
                            "parent_tool_call_id": "tc_task_alias",
                            "parent_request_id": "req_native_tool_parity",
                            "child_session_id": "agent_worker",
                            "child_request_id": "req_child",
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                11,
                Some("2026-03-22T14:36:02Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_task_alias",
                    "status": "succeeded",
                    "output_summary": "task_id: agent_worker\nrequest_id: req_child",
                    "output_digest": "digest-task-alias-output",
                    "output_json": {
                        "description": "audit transcript parity",
                        "profile": "researcher",
                        "mode": "foreground",
                        "status": "completed",
                        "duration_ms": 1600,
                        "result_summary": "Found the inline transcript path.",
                        "child_tool_call_count": 3,
                        "child_session_id": "agent_worker",
                        "child_request_id": "req_child",
                    },
                    "metadata": {
                        "canonical_tool_id": "task",
                        "lineage": {
                            "parent_tool_call_id": "tc_task_alias",
                            "parent_request_id": "req_native_tool_parity",
                            "child_session_id": "agent_worker",
                            "child_request_id": "req_child",
                        },
                        "timing": {
                            "elapsed_ms": 1600,
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                12,
                Some("2026-03-22T14:37:10Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_fetch",
                    "tool_id": "webfetch",
                    "args_summary": "{\"url\":\"https://example.test/report.pdf\",\"format\":\"markdown\"}",
                    "args_digest": "digest-fetch-args",
                    "metadata": {
                        "canonical_tool_id": "webfetch",
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                13,
                Some("2026-03-22T14:37:12Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_fetch",
                    "status": "succeeded",
                    "output_summary": "report ready\npage count: 2\nformat: pdf\nstored inline artifact",
                    "output_digest": "digest-fetch-output",
                    "output_json": Value::Null,
                    "metadata": {
                        "canonical_tool_id": "webfetch",
                        "artifact_refs": [
                            {
                                "path": "artifacts/toolcalls/tc-fetch/web.fetch.pdf",
                                "digest": "digest-fetch-artifact",
                            }
                        ],
                        "timing": {
                            "elapsed_ms": 2400,
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                14,
                Some("2026-03-22T14:37:20Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_docs_rs",
                    "tool_id": "mcp.docs-rs.search_in_crate",
                    "args_summary": "{\"crate_name\":\"ratatui\",\"query\":\"Layout\"}",
                    "args_digest": "digest-docs-rs-search-args",
                    "metadata": {
                        "canonical_tool_id": "mcp.docs-rs.search_in_crate",
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                15,
                Some("2026-03-22T14:37:21Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_docs_rs",
                    "status": "succeeded",
                    "output_summary": "struct Layout\nmodule layout\ntrait WidgetRef",
                    "output_digest": "digest-docs-rs-search-output",
                    "output_json": {
                        "server": {
                            "id": "docs-rs",
                            "transport": "stdio",
                        },
                        "payload": {
                            "tool": "search_in_crate",
                            "arguments": {
                                "crate_name": "ratatui",
                                "query": "Layout",
                            },
                            "result": {
                                "content": [{"type": "text", "text": "struct Layout\nmodule layout\ntrait WidgetRef"}],
                                "isError": false,
                            }
                        }
                    },
                    "metadata": {
                        "canonical_tool_id": "mcp.docs-rs.search_in_crate",
                        "timing": {
                            "elapsed_ms": 650,
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                16,
                Some("2026-03-22T14:37:30Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_shell_fail",
                    "tool_id": "bash",
                    "args_summary": "{\"command\":\"cargo test -p harness-tui\",\"workdir\":\"/workspace\",\"description\":\"Run harness-tui tests\"}",
                    "args_digest": "digest-shell-fail-args",
                    "metadata": {
                        "canonical_tool_id": "bash",
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                17,
                Some("2026-03-22T14:37:31Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_shell_fail",
                    "status": "failed",
                    "output_summary": "exit code: 1\nstderr: snapshot mismatch",
                    "output_digest": Value::Null,
                    "output_json": Value::Null,
                    "metadata": {
                        "canonical_tool_id": "bash",
                        "timing": {
                            "elapsed_ms": 900,
                        }
                    }
                }),
            ),
            session_event_with_ts(
                run_id,
                18,
                Some("2026-03-22T14:37:40Z"),
                event_actor("worker", Some("agent_000001")),
                Some("req_native_tool_parity"),
                "provider_stream_delta",
                json!({
                    "request_id": "req_native_tool_parity",
                    "delta": "Inline transcript parity is easier to scan now.",
                }),
            ),
            session_event_with_ts(
                run_id,
                19,
                Some("2026-03-22T14:37:41Z"),
                event_actor("system", Some("coordinator")),
                Some("req_native_tool_parity"),
                "provider_request_finished",
                json!({
                    "request_id": "req_native_tool_parity",
                    "finish_reason": "stop",
                    "output_digest": "digest-native-tool-parity-response",
                }),
            ),
            session_event_with_ts(
                run_id,
                20,
                Some("2026-03-22T14:37:42Z"),
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "native tool parity captured",
                }),
            ),
        ],
    )
}

fn write_active_blocked_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
    write_session_fixture(
        session_dir,
        run_id,
        &[
            session_event(
                run_id,
                1,
                event_actor("system", Some("coordinator")),
                None,
                "run_started",
                json!({
                    "run_name": "interactive",
                    "workspace_root": "/workspace/project",
                }),
            ),
            session_event(
                run_id,
                2,
                event_actor("system", Some("coordinator")),
                None,
                "agent_spawned",
                json!({
                    "agent_id": "agent_000001",
                    "profile": "worker",
                    "parent_agent_id": Value::Null,
                }),
            ),
            session_event(
                run_id,
                3,
                event_actor("worker", Some("agent_000001")),
                Some("req_000001"),
                "provider_request_started",
                json!({
                    "request_id": "req_000001",
                    "provider_id": "default",
                    "model_id": "model-1",
                    "prompt_summary": "unfinished prompt",
                    "request_digest": "digest-active-request",
                }),
            ),
            session_event(
                run_id,
                4,
                event_actor("system", Some("coordinator")),
                None,
                "task_scheduled",
                json!({
                    "task_id": "task_000001",
                    "state": "started",
                    "queue_key": "tool:bash",
                }),
            ),
            session_event(
                run_id,
                5,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "segment closed while work remained active",
                }),
            ),
        ],
    )
}

fn write_corrupt_blocked_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(run_dir.join("artifacts")).expect("create corrupt fixture artifacts dir");

    let valid_first_line = serde_json::to_string(&session_event(
        run_id,
        1,
        event_actor("system", Some("coordinator")),
        None,
        "run_started",
        json!({
            "run_name": "interactive",
            "workspace_root": "/workspace/project",
        }),
    ))
    .expect("serialize corrupt fixture first line");

    fs::write(
        run_dir.join("events.jsonl"),
        format!("{valid_first_line}\n{{bad-json}}\n"),
    )
    .expect("write corrupt fixture events");
    run_dir
}

fn write_replay_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
    let run_dir = write_session_fixture(
        session_dir,
        run_id,
        &[
            session_event(
                run_id,
                1,
                event_actor("system", Some("coordinator")),
                None,
                "run_started",
                json!({
                    "run_name": "interactive",
                    "workspace_root": "/workspace/project",
                }),
            ),
            session_event(
                run_id,
                2,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "done",
                }),
            ),
        ],
    );
    fs::write(
        run_dir.join("meta.json"),
        json!({
            "run_id": run_id,
            "run_name": "interactive",
            "workspace_root": "/workspace/project",
            "config_digest": "fixture",
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
    .expect("write replay fixture metadata");
    run_dir
}

fn write_session_fixture(session_dir: &Path, run_id: &str, events: &[Value]) -> PathBuf {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(run_dir.join("artifacts")).expect("create session fixture artifacts dir");

    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("serialize session fixture event");
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(run_dir.join("events.jsonl"), body).expect("write session fixture events");
    run_dir
}

fn sample_diff_artifact() -> &'static str {
    "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n"
}

fn write_diff_artifact(run_dir: &Path, relative_path: &str, diff_content: &str) {
    let diff_path = run_dir.join(relative_path);
    if let Some(parent) = diff_path.parent() {
        fs::create_dir_all(parent).expect("create diff artifact parent dir");
    }
    fs::write(diff_path, diff_content).expect("write diff artifact fixture");
}

fn event_actor(kind: &str, actor_id: Option<&str>) -> Value {
    json!({
        "kind": kind,
        "agent_id": actor_id,
    })
}

fn session_event(
    run_id: &str,
    seq: u64,
    actor: Value,
    correlation_id: Option<&str>,
    event_type: &str,
    data: Value,
) -> Value {
    session_event_with_ts(run_id, seq, None, actor, correlation_id, event_type, data)
}

fn session_event_with_ts(
    run_id: &str,
    seq: u64,
    ts: Option<&str>,
    actor: Value,
    correlation_id: Option<&str>,
    event_type: &str,
    data: Value,
) -> Value {
    json!({
        "schema_version": 1,
        "event_id": format!("evt-{seq:04}"),
        "seq": seq,
        "run_id": run_id,
        "mono_ms": seq,
        "ts": ts,
        "actor": actor,
        "correlation_id": correlation_id,
        "causation_id": Value::Null,
        "stream_key": format!("run:{run_id}"),
        "payload": {
            "event_type": event_type,
            "data": data,
        },
    })
}

#[cfg(target_os = "windows")]
fn binary_name(name: &str) -> String {
    format!("{name}.exe")
}

#[cfg(not(target_os = "windows"))]
fn binary_name(name: &str) -> String {
    name.to_string()
}
