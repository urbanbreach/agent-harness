use font8x8::unicode::{BasicFonts, BlockFonts, BoxFonts, LatinFonts, UnicodeFonts};
use fontdue::{Font, FontSettings};
use image::{imageops::FilterType, Rgb, RgbImage};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::cmp;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError},
    Arc, OnceLock,
};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vt100::{Color as VtColor, Parser as VtParser};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const MARKER_TIMEOUT: Duration = Duration::from_secs(6);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const STABLE_WINDOW: Duration = Duration::from_millis(180);
const STABLE_TIMEOUT: Duration = Duration::from_secs(2);
const GLYPH_WIDTH: u32 = 8;
const GLYPH_HEIGHT: u32 = 8;
const GLYPH_VERTICAL_SCALE: u32 = 2;
const RASTER_SCALE: u32 = 4;
const RASTER_CELL_WIDTH: u32 = GLYPH_WIDTH * RASTER_SCALE;
const RASTER_CELL_HEIGHT: u32 = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * RASTER_SCALE;
const PRIMARY_SIGNOFF_COLS: u16 = 160;
const PRIMARY_SIGNOFF_ROWS: u16 = 48;
const CELL_WIDTH: u32 = 16;
const CELL_HEIGHT: u32 = 30;
const DEFAULT_FG: [u8; 3] = [216, 216, 216];
const DEFAULT_BG: [u8; 3] = [18, 18, 18];
const ANTI_ALIAS_FONT_SIZE_FACTOR: f32 = 0.72;
const STARTUP_LAUNCHER_READY_MARKER: &str = "startup launcher ready";
const STARTUP_LAUNCHER_GUIDANCE_MARKER: &str = "Dispatch a new run";
const STARTUP_COMMAND_PALETTE_MARKER: &str = "Command palette";
const STARTUP_CONTINUE_HISTORY_MARKER: &str = "Continue session";
const STARTUP_CONTINUE_SCOPE_MARKER: &str = "Interactive histories";
const STARTUP_REPLAY_HISTORY_MARKER: &str = "Replay session";
const REPLAY_READY_MARKER: &str = "q quit";
const NEXT_ACTION_MARKER: &str = "Next action";
const CONTINUED_LIVE_RUN_MARKER: &str = "Continued live run";
const STARTUP_LIFECYCLE_MARKERS: &[&str] = &[
    STARTUP_LAUNCHER_READY_MARKER,
    STARTUP_LAUNCHER_GUIDANCE_MARKER,
    "New session",
    "Continue session",
    "Replay session",
];
const POST_RUN_HANDOFF_MARKERS: &[&str] = &[
    NEXT_ACTION_MARKER,
    "Continue this session",
    "Replay this run",
    "Start another session",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct OfflineVisualEvidenceContract {
    family: &'static str,
    state: &'static str,
    png: &'static str,
    snapshot: &'static str,
}

const OFFLINE_VISUAL_EVIDENCE_CONTRACTS: &[OfflineVisualEvidenceContract] = &[
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "happy_path",
        png: "pty_startup_launcher_ready.png",
        snapshot: "pty_startup_launcher_ready",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "happy_path",
        png: "pty_startup_command_palette.png",
        snapshot: "pty_startup_command_palette",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "happy_path",
        png: "pty_startup_continue_history.png",
        snapshot: "pty_startup_continue_history",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "happy_path",
        png: "pty_startup_replay_history.png",
        snapshot: "pty_startup_replay_history",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "responsive",
        png: "pty_startup_quarter_tile.png",
        snapshot: "pty_startup_quarter_tile",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "responsive",
        png: "pty_startup_split_tall.png",
        snapshot: "pty_startup_split_tall",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "responsive",
        png: "pty_startup_split_tier.png",
        snapshot: "pty_startup_split_tier",
    },
    OfflineVisualEvidenceContract {
        family: "startup",
        state: "responsive",
        png: "pty_startup_six_window.png",
        snapshot: "pty_startup_six_window",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "failure_path",
        png: "pty_continue_rejected_active.png",
        snapshot: "pty_continue_rejected_active",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "failure_path",
        png: "pty_continue_rejected_unrestorable.png",
        snapshot: "pty_continue_rejected_unrestorable",
    },
    OfflineVisualEvidenceContract {
        family: "permission",
        state: "happy_path",
        png: "pty_permission_requested.png",
        snapshot: "pty_permission_requested",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "happy_path",
        png: "pty_interactive_type_first_startup.png",
        snapshot: "pty_interactive_type_first_startup",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "happy_path",
        png: "pty_interactive_prompt_stream.png",
        snapshot: "pty_interactive_prompt_stream",
    },
    OfflineVisualEvidenceContract {
        family: "live_details",
        state: "happy_path",
        png: "pty_continue_live_details_primary.png",
        snapshot: "pty_continue_live_details_primary",
    },
    OfflineVisualEvidenceContract {
        family: "live_details",
        state: "responsive",
        png: "pty_continue_details_quarter_tile.png",
        snapshot: "pty_continue_details_quarter_tile",
    },
    OfflineVisualEvidenceContract {
        family: "live_details",
        state: "responsive",
        png: "pty_continue_details_split_tall.png",
        snapshot: "pty_continue_details_split_tall",
    },
    OfflineVisualEvidenceContract {
        family: "live_details",
        state: "responsive",
        png: "pty_continue_details_split_tier.png",
        snapshot: "pty_continue_details_split_tier",
    },
    OfflineVisualEvidenceContract {
        family: "live_details",
        state: "responsive",
        png: "pty_continue_details_six_window.png",
        snapshot: "pty_continue_details_six_window",
    },
    OfflineVisualEvidenceContract {
        family: "live_secondary",
        state: "happy_path",
        png: "pty_continue_live_events.png",
        snapshot: "pty_continue_live_events",
    },
    OfflineVisualEvidenceContract {
        family: "live_secondary",
        state: "happy_path",
        png: "pty_continue_live_help.png",
        snapshot: "pty_continue_live_help",
    },
    OfflineVisualEvidenceContract {
        family: "live_secondary",
        state: "happy_path",
        png: "pty_continue_live_diff_primary.png",
        snapshot: "pty_continue_live_diff_primary",
    },
    OfflineVisualEvidenceContract {
        family: "post_run",
        state: "happy_path",
        png: "pty_run_finished.png",
        snapshot: "pty_run_finished",
    },
    OfflineVisualEvidenceContract {
        family: "post_run",
        state: "responsive",
        png: "pty_post_run_quarter_tile.png",
        snapshot: "pty_post_run_quarter_tile",
    },
    OfflineVisualEvidenceContract {
        family: "post_run",
        state: "responsive",
        png: "pty_post_run_split_tall.png",
        snapshot: "pty_post_run_split_tall",
    },
    OfflineVisualEvidenceContract {
        family: "post_run",
        state: "responsive",
        png: "pty_post_run_split_tier.png",
        snapshot: "pty_post_run_split_tier",
    },
    OfflineVisualEvidenceContract {
        family: "post_run",
        state: "responsive",
        png: "pty_post_run_six_window.png",
        snapshot: "pty_post_run_six_window",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "happy_path",
        png: "pty_continue_quiescent_session.png",
        snapshot: "pty_continue_quiescent_session",
    },
    OfflineVisualEvidenceContract {
        family: "replay",
        state: "happy_path",
        png: "pty_replay_from_handoff_read_only.png",
        snapshot: "pty_replay_from_handoff_read_only",
    },
    OfflineVisualEvidenceContract {
        family: "replay",
        state: "failure_path",
        png: "pty_replay_read_only.png",
        snapshot: "pty_replay_read_only",
    },
    OfflineVisualEvidenceContract {
        family: "replay_secondary",
        state: "happy_path",
        png: "pty_replay_diff_tab.png",
        snapshot: "pty_replay_diff_tab",
    },
    OfflineVisualEvidenceContract {
        family: "replay_secondary",
        state: "happy_path",
        png: "pty_replay_help_tab.png",
        snapshot: "pty_replay_help_tab",
    },
    OfflineVisualEvidenceContract {
        family: "replay_secondary",
        state: "responsive",
        png: "pty_replay_events_six_window.png",
        snapshot: "pty_replay_events_six_window",
    },
];

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
        ready_marker: "Composer",
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
        draft_snapshot_markers: &["Hello from PTY", "Composer"],
        focus_capture: FocusCapture::anchored("Hello world", 28, 6),
        snapshot_markers: &["Hello world", "Composer · ready"],
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
        marker: "Permission Requested",
        draft_text: "keep this draft",
        approve_key: b'a',
        follow_toggle_key: b' ',
        rewind_key: b'k',
        rewind_steps: 64,
        focus_capture: FocusCapture::anchored_exact("Permission Requested", 24, 1),
        snapshot_markers: &[
            "Permission Requested",
            "FAIL CLOSED",
            "keep this draft",
            "[d] deny",
            "Composer · disabled",
        ],
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

struct SpawnedHarnessPty {
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: VtParser,
}

fn spawn_harness_tui(
    geometry: PtyGeometry,
    session_dir: &Path,
    configure: impl FnOnce(&mut CommandBuilder),
) -> SpawnedHarnessPty {
    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(geometry.pty_size())
        .expect("open pty pair");

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    configure(&mut command);
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(repo_root);
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn harness tui command");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let writer = pair.master.take_writer().expect("take pty writer");

    SpawnedHarnessPty {
        child,
        writer,
        output_rx: spawn_reader_thread(reader),
        parser: geometry.parser(),
    }
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

fn open_startup_launcher_action(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    down_steps: usize,
    marker: &str,
) -> String {
    move_list_selection(writer, down_steps);
    send_key(writer, b'\r').expect("select startup launcher action");
    wait_for_screen_contains(parser, output_rx, marker, MARKER_TIMEOUT)
        .expect("wait for startup launcher action screen")
}

fn select_list_action(writer: &mut dyn Write, down_steps: usize) {
    move_list_selection(writer, down_steps);
    send_key(writer, b'\r').expect("select focused list action");
}

fn open_live_details_drawer(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
) -> String {
    send_key(writer, b'\t').expect("focus transcript before opening details drawer");
    send_key(writer, b'i').expect("toggle live details drawer");
    wait_for_screen_contains(parser, output_rx, "Request ID:", MARKER_TIMEOUT)
        .expect("wait for live details drawer")
}

fn open_live_secondary_tab(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    tab_key: u8,
    marker: &str,
) -> String {
    send_key(writer, tab_key).expect("switch live secondary tab");
    wait_for_screen_contains(parser, output_rx, marker, MARKER_TIMEOUT)
        .expect("wait for live secondary tab")
}

fn spawn_resumed_quiescent_session(
    geometry: PtyGeometry,
    session_dir: &Path,
    config_path: &Path,
    run_id: &str,
) -> SpawnedHarnessPty {
    let mut harness = spawn_harness_tui(geometry, session_dir, |command| {
        command.arg("--config");
        command.arg(config_path.to_string_lossy().to_string());
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher ready before continuing quiescent session");

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
        NEXT_ACTION_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for post-run handoff before reopening quiescent session");
    send_key(harness.writer.as_mut(), b'\r').expect("reopen quiescent session live");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        &format!("Continued · run {run_id}"),
        STARTUP_TIMEOUT,
    )
    .expect("wait for continued live session shell");

    harness
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

    let lifecycle_session_dir = create_temp_session_dir();
    let lifecycle_run_id = "run_resume_quiescent";
    let lifecycle_run_dir =
        write_quiescent_resume_fixture(&lifecycle_session_dir, lifecycle_run_id);
    let lifecycle_events_path = lifecycle_run_dir.join("events.jsonl");
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut lifecycle_harness = spawn_harness_tui(
        PtyGeometry::PRIMARY_SIGNOFF,
        &lifecycle_session_dir,
        |command| {
            command.arg("--mock");
        },
    );

    let startup_screen = wait_for_screen_contains(
        &mut lifecycle_harness.parser,
        &lifecycle_harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for lifecycle startup launcher ready");
    let startup_visual = capture_visual_checkpoint(
        "mock_lifecycle_startup_launcher_ready",
        &lifecycle_harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_LAUNCHER_READY_MARKER, 40, 2),
    )
    .expect("capture lifecycle startup launcher checkpoint image");
    insta::assert_snapshot!(
        "pty_mock_lifecycle_startup_launcher_ready",
        checkpoint_visual_snapshot(&startup_screen, STARTUP_LIFECYCLE_MARKERS, &startup_visual)
    );

    let history_screen = open_startup_launcher_action(
        &mut lifecycle_harness.parser,
        &lifecycle_harness.output_rx,
        lifecycle_harness.writer.as_mut(),
        1,
        "continue ready",
    );
    let history_visual = capture_visual_checkpoint(
        "mock_lifecycle_continue_history",
        &lifecycle_harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("continue ready", 28, 3),
    )
    .expect("capture startup continue history image");
    insta::assert_snapshot!(
        "pty_mock_lifecycle_continue_history",
        checkpoint_visual_snapshot(
            &history_screen,
            &[
                STARTUP_CONTINUE_HISTORY_MARKER,
                STARTUP_CONTINUE_SCOPE_MARKER,
                "continue ready",
            ],
            &history_visual,
        )
    );

    send_key(lifecycle_harness.writer.as_mut(), b'\r')
        .expect("select continued quiescent session from startup history");
    let continued_screen = wait_for_screen_contains(
        &mut lifecycle_harness.parser,
        &lifecycle_harness.output_rx,
        NEXT_ACTION_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for post-run handoff after continuing quiescent session");
    assert!(
        continued_screen.contains("Continue this session"),
        "continued quiescent session should expose the post-run continue action"
    );

    select_list_action(lifecycle_harness.writer.as_mut(), 1);
    wait_for_screen_contains(
        &mut lifecycle_harness.parser,
        &lifecycle_harness.output_rx,
        REPLAY_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell launched from post-run handoff");

    let events_during_replay_before_submit = fs::read_to_string(&lifecycle_events_path)
        .expect("read lifecycle run events after replay handoff enters replay mode");

    send_key(lifecycle_harness.writer.as_mut(), b'\t').expect("focus replay details pane");
    send_key(lifecycle_harness.writer.as_mut(), b'\t').expect("attempt replay prompt focus");
    type_text(lifecycle_harness.writer.as_mut(), "blocked in replay");
    send_key(lifecycle_harness.writer.as_mut(), b'\r').expect("attempt replay submit from handoff");

    let replay_screen = wait_for_screen_contains(
        &mut lifecycle_harness.parser,
        &lifecycle_harness.output_rx,
        REPLAY_READY_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for stable replay screen after handoff submit attempt");
    let replay_visual = capture_visual_checkpoint(
        "replay_from_handoff_read_only",
        &lifecycle_harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(REPLAY_READY_MARKER, 20, 2),
    )
    .expect("capture replay-from-handoff read-only image");
    insta::assert_snapshot!(
        "pty_replay_from_handoff_read_only",
        checkpoint_visual_snapshot(
            &replay_screen,
            &[REPLAY_READY_MARKER, lifecycle_run_id, "Replay is read-only"],
            &replay_visual,
        )
    );

    let events_after_replay_submit = fs::read_to_string(&lifecycle_events_path)
        .expect("read lifecycle run events after replay submit attempt");
    assert_eq!(
        events_after_replay_submit, events_during_replay_before_submit,
        "replay mode launched from the lifecycle handoff must stay read-only while active"
    );

    send_key(lifecycle_harness.writer.as_mut(), b'q').expect("close replay and return to startup");
    wait_for_screen_contains(
        &mut lifecycle_harness.parser,
        &lifecycle_harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher after closing replay from handoff");
    let events_after_replay_close = fs::read_to_string(&lifecycle_events_path)
        .expect("read lifecycle run events after closing replay");
    assert_eq!(
        events_after_replay_close, events_during_replay_before_submit,
        "closing replay launched from the lifecycle handoff must not append to events.jsonl"
    );
    lifecycle_harness
        .child
        .kill()
        .expect("terminate lifecycle launcher after replay signoff");
    std::mem::forget(lifecycle_harness.child);

    let scenario_session_dir = create_temp_session_dir();
    let mut scenario_harness = spawn_harness_tui(
        PtyGeometry::MINIMUM_SIGNOFF,
        &scenario_session_dir,
        |command| {
            command.arg("--scenario");
            command.arg("golden_path_interactive");
        },
    );

    LIVE_STATE_FIXTURES
        .permission
        .write_preserved_draft(scenario_harness.writer.as_mut())
        .expect("queue preserved draft before permission prompt");

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
    insta::assert_snapshot!(
        "pty_permission_requested",
        checkpoint_visual_snapshot(
            &permission_checkpoint,
            LIVE_STATE_FIXTURES.permission.snapshot_markers,
            &permission_visual
        )
    );

    LIVE_STATE_FIXTURES
        .permission
        .approve(scenario_harness.writer.as_mut())
        .expect("send approve key");

    let run_finished_checkpoint = wait_for_screen_contains(
        &mut scenario_harness.parser,
        &scenario_harness.output_rx,
        NEXT_ACTION_MARKER,
        MARKER_TIMEOUT,
    )
    .expect("wait for post-run handoff marker");
    let run_finished_visual = capture_visual_checkpoint(
        "run_finished",
        &scenario_harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(NEXT_ACTION_MARKER, 28, 1),
    )
    .expect("capture run finished handoff checkpoint image");
    insta::assert_snapshot!(
        "pty_run_finished",
        checkpoint_visual_snapshot(
            &run_finished_checkpoint,
            POST_RUN_HANDOFF_MARKERS,
            &run_finished_visual
        )
    );

    scenario_harness
        .child
        .kill()
        .expect("terminate scenario lifecycle handoff after signoff");
    std::mem::forget(scenario_harness.child);
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

    let harness_bin = resolve_harness_bin();
    let repo_root = repo_root();
    let session_dir = create_temp_session_dir();
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
    let geometry = PtyGeometry::MINIMUM_SIGNOFF;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(geometry.pty_size())
        .expect("open pty pair");

    let mut command = CommandBuilder::new(harness_bin.to_string_lossy().as_ref());
    command.arg("tui");
    command.arg("--config");
    command.arg(config_path.to_string_lossy().to_string());
    command.arg("--deterministic");
    command.arg("--session-dir");
    command.arg(session_dir.to_string_lossy().to_string());
    command.cwd(repo_root);
    configure_deterministic_env(&mut command);

    let child = pair
        .slave
        .spawn_command(command)
        .expect("spawn harness tui command");
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().expect("clone pty reader");
    let mut writer = pair.master.take_writer().expect("take pty writer");
    let output_rx = spawn_reader_thread(reader);
    let mut parser = geometry.parser();
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
    let startup_visual = capture_visual_checkpoint(
        "interactive_type_first_startup",
        &parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.draft.draft_focus_capture,
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
    let prompt_visual = capture_visual_checkpoint(
        "interactive_prompt_stream",
        &parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.draft.focus_capture,
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

    let mut child = child;
    child.kill().expect("terminate interactive tui child");
    std::mem::forget(child);
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
    insta::assert_snapshot!(
        "pty_startup_launcher_ready",
        checkpoint_visual_snapshot(
            &screen_contents(&harness.parser),
            STARTUP_LIFECYCLE_MARKERS,
            &startup_visual
        )
    );

    let palette_screen = open_startup_command_palette(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
    );
    let palette_visual = capture_visual_checkpoint(
        "startup_command_palette",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_COMMAND_PALETTE_MARKER, 28, 5),
    )
    .expect("capture startup command palette image");
    insta::assert_snapshot!(
        "pty_startup_command_palette",
        checkpoint_visual_snapshot(
            &palette_screen,
            &[
                STARTUP_COMMAND_PALETTE_MARKER,
                "New session",
                "Continue session",
            ],
            &palette_visual,
        )
    );

    let replay_history_screen = open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    let replay_history_visual = capture_visual_checkpoint(
        "startup_replay_history",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_REPLAY_HISTORY_MARKER, 28, 3),
    )
    .expect("capture startup replay history image");
    insta::assert_snapshot!(
        "pty_startup_replay_history",
        checkpoint_visual_snapshot(
            &replay_history_screen,
            &[STARTUP_REPLAY_HISTORY_MARKER, "interactive", "read-only"],
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

    let history_screen = open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "resume",
        "continue ready",
    );
    let history_visual = capture_visual_checkpoint(
        "startup_continue_history",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("continue ready", 28, 3),
    )
    .expect("capture startup resume history image");
    insta::assert_snapshot!(
        "pty_startup_continue_history",
        checkpoint_visual_snapshot(
            &history_screen,
            &[
                STARTUP_CONTINUE_HISTORY_MARKER,
                "interactive",
                "continue ready"
            ],
            &history_visual,
        )
    );

    send_key(harness.writer.as_mut(), b'\r').expect("continue selected quiescent session");
    let handoff_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        NEXT_ACTION_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for quiescent-session handoff before resuming live session");
    assert!(
        !handoff_screen.contains(&format!("Continued · run {run_id}")),
        "continued live header must stay hidden until the handoff is confirmed"
    );
    send_key(harness.writer.as_mut(), b'\r').expect("resume quiescent session from handoff");
    let continued_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        &format!("Continued · run {run_id}"),
        STARTUP_TIMEOUT,
    )
    .expect("wait for continued-session handoff after reopening run");
    let continued_visual = capture_visual_checkpoint(
        "continue_quiescent_session",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(CONTINUED_LIVE_RUN_MARKER, 18, 1),
    )
    .expect("capture continued session checkpoint image");
    insta::assert_snapshot!(
        "pty_continue_quiescent_session",
        checkpoint_visual_snapshot(
            &continued_screen,
            &[
                &format!("Continued · run {run_id}"),
                CONTINUED_LIVE_RUN_MARKER,
                "Composer",
                "Same run reopened live",
            ],
            &continued_visual,
        )
    );

    let details_screen = open_live_details_drawer(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
    );
    let details_visual = capture_visual_checkpoint(
        "continue_live_details_primary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Request ID:", 28, 6),
    )
    .expect("capture primary live details drawer image");
    insta::assert_snapshot!(
        "pty_continue_live_details_primary",
        checkpoint_visual_snapshot(
            &details_screen,
            &[
                "Request ID:",
                "Provider:",
                "Model:",
                "Prompt summary:",
                "historical question",
            ],
            &details_visual,
        )
    );

    let events_screen = open_live_secondary_tab(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        b'2',
        "Event log",
    );
    let events_visual = capture_visual_checkpoint(
        "continue_live_events",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Event log", 24, 6),
    )
    .expect("capture continued-session events tab image");
    insta::assert_snapshot!(
        "pty_continue_live_events",
        checkpoint_visual_snapshot(
            &events_screen,
            &["Event log", "Event details", "recorded", "selected seq"],
            &events_visual,
        )
    );

    let help_screen = open_live_secondary_tab(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        b'4',
        "Keyboard Shortcuts:",
    );
    let help_visual = capture_visual_checkpoint(
        "continue_live_help",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Keyboard Shortcuts:", 26, 10),
    )
    .expect("capture continued-session help tab image");
    insta::assert_snapshot!(
        "pty_continue_live_help",
        checkpoint_visual_snapshot(
            &help_screen,
            &[
                "Keyboard Shortcuts:",
                "Live surfaces:",
                "Toggle details drawer",
                "Open Events surface",
                "Open Help surface",
            ],
            &help_visual,
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
async fn pty_e2e_live_details_drawer_stays_usable_across_window_sizes() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    for (geometry, snapshot_name, artifact_name) in [
        (
            PtyGeometry::MINIMUM_SIGNOFF,
            "pty_continue_details_quarter_tile",
            "continue_details_quarter_tile",
        ),
        (
            PtyGeometry::HALF_SCREEN_SPLIT,
            "pty_continue_details_split_tall",
            "continue_details_split_tall",
        ),
        (
            PtyGeometry::SPLIT_TIER_WINDOW,
            "pty_continue_details_split_tier",
            "continue_details_split_tier",
        ),
        (
            PtyGeometry::SIX_WINDOW_DENSE,
            "pty_continue_details_six_window",
            "continue_details_six_window",
        ),
    ] {
        let session_dir = create_temp_session_dir();
        let run_id = "run_resume_quiescent";
        write_quiescent_resume_fixture(&session_dir, run_id);
        let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
        let mut harness =
            spawn_resumed_quiescent_session(geometry, &session_dir, &config_path, run_id);

        let details_screen = open_live_details_drawer(
            &mut harness.parser,
            &harness.output_rx,
            harness.writer.as_mut(),
        );
        let details_visual = capture_visual_checkpoint(
            artifact_name,
            &harness.parser,
            &visual_dir,
            FocusCapture::anchored_exact("Request ID:", 24, 6),
        )
        .expect("capture responsive live details drawer image");

        insta::assert_snapshot!(
            snapshot_name,
            checkpoint_visual_snapshot(
                &details_screen,
                &[
                    "Request ID:",
                    "Provider:",
                    "Model:",
                    "Prompt summary:",
                    "historical question",
                ],
                &details_visual,
            )
        );

        harness
            .child
            .kill()
            .expect("terminate responsive continued-session harness");
        std::mem::forget(harness.child);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_live_diff_tab_covers_primary_live_secondary_surface() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let session_dir = create_temp_session_dir();
    let run_id = "run_resume_quiescent";
    write_quiescent_resume_fixture(&session_dir, run_id);
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
    let mut harness = spawn_resumed_quiescent_session(
        PtyGeometry::PRIMARY_SIGNOFF,
        &session_dir,
        &config_path,
        run_id,
    );

    send_key(harness.writer.as_mut(), b'\t').expect("focus details before opening live diff tab");
    let diff_screen = open_live_secondary_tab(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        b'3',
        "diff artifact missing:",
    );
    let diff_visual = capture_visual_checkpoint(
        "continue_live_diff_primary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("diff artifact missing:", 28, 6),
    )
    .expect("capture live diff tab image");

    insta::assert_snapshot!(
        "pty_continue_live_diff_primary",
        checkpoint_visual_snapshot(
            &diff_screen,
            &["artifact view · seq", "diff artifact missing:", "Diff",],
            &diff_visual,
        )
    );

    harness.child.kill().expect("terminate live diff harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_replay_secondary_surfaces_cover_diff_and_help() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let run_id = "run_replay_diff";
    write_replay_diff_fixture(&session_dir, run_id);
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
    .expect("wait for startup launcher before opening replay secondary surfaces");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(harness.writer.as_mut(), b'\r').expect("select replay diff fixture session");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell before opening secondary tabs");

    send_key(harness.writer.as_mut(), b'\t').expect("focus replay details surface before diff tab");

    let diff_screen = open_live_secondary_tab(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        b'3',
        "diff artifact missing:",
    );
    let diff_visual = capture_visual_checkpoint(
        "replay_diff_tab",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("diff artifact missing:", 26, 6),
    )
    .expect("capture replay diff tab image");
    insta::assert_snapshot!(
        "pty_replay_diff_tab",
        checkpoint_visual_snapshot(
            &diff_screen,
            &[
                "diff artifact missing:",
                "artifact view · seq",
                "read-only",
                run_id,
            ],
            &diff_visual,
        )
    );

    let help_screen = open_live_secondary_tab(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        b'4',
        "Keyboard Shortcuts:",
    );
    let help_visual = capture_visual_checkpoint(
        "replay_help_tab",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Keyboard Shortcuts:", 26, 12),
    )
    .expect("capture replay help tab image");
    insta::assert_snapshot!(
        "pty_replay_help_tab",
        checkpoint_visual_snapshot(
            &help_screen,
            &[
                "Keyboard Shortcuts:",
                "Replay surfaces:",
                "Open Diff surface",
                "Reload session",
                "Quit",
            ],
            &help_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate replay secondary-surface harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_replay_events_cover_dense_secondary_layout() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let run_id = "run_replay_diff";
    write_replay_diff_fixture(&session_dir, run_id);
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::SIX_WINDOW_DENSE, &session_dir, |command| {
        command.arg("--mock");
    });

    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        STARTUP_LAUNCHER_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for startup launcher before replay events tab");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(harness.writer.as_mut(), b'\r').expect("select replay session for dense events tab");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell before opening dense events tab");

    send_key(harness.writer.as_mut(), b'\t').expect("focus replay details before events tab");
    let events_screen = open_live_secondary_tab(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        b'2',
        "Event log",
    );
    let events_visual = capture_visual_checkpoint(
        "replay_events_six_window",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Event log", 20, 6),
    )
    .expect("capture dense replay events tab image");

    insta::assert_snapshot!(
        "pty_replay_events_six_window",
        checkpoint_visual_snapshot(
            &events_screen,
            &["Event log", "Event details", "recorded", "selected seq"],
            &events_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate dense replay events harness");
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

    for (geometry, snapshot_name, artifact_name) in [
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

        let startup_screen = wait_for_screen_contains(
            &mut harness.parser,
            &harness.output_rx,
            STARTUP_LAUNCHER_READY_MARKER,
            STARTUP_TIMEOUT,
        )
        .expect("wait for startup launcher in alternate window shape");
        let startup_visual = capture_visual_checkpoint(
            artifact_name,
            &harness.parser,
            &visual_dir,
            FocusCapture::anchored_exact(STARTUP_LAUNCHER_READY_MARKER, 40, 2),
        )
        .expect("capture alternate startup launcher image");

        insta::assert_snapshot!(
            snapshot_name,
            checkpoint_visual_snapshot(&startup_screen, STARTUP_LIFECYCLE_MARKERS, &startup_visual,)
        );

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
fn pty_e2e_post_run_handoff_stays_usable_in_split_and_dense_windows() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    for (geometry, snapshot_name, artifact_name) in [
        (
            PtyGeometry::MINIMUM_SIGNOFF,
            "pty_post_run_quarter_tile",
            "post_run_quarter_tile",
        ),
        (
            PtyGeometry::HALF_SCREEN_SPLIT,
            "pty_post_run_split_tall",
            "post_run_split_tall",
        ),
        (
            PtyGeometry::SPLIT_TIER_WINDOW,
            "pty_post_run_split_tier",
            "post_run_split_tier",
        ),
        (
            PtyGeometry::SIX_WINDOW_DENSE,
            "pty_post_run_six_window",
            "post_run_six_window",
        ),
    ] {
        let session_dir = create_temp_session_dir();
        let mut harness = spawn_harness_tui(geometry, &session_dir, |command| {
            command.arg("--scenario");
            command.arg("golden_path_interactive");
        });

        LIVE_STATE_FIXTURES
            .permission
            .write_preserved_draft(harness.writer.as_mut())
            .expect("queue preserved draft before narrow post-run handoff");

        wait_for_screen_contains(
            &mut harness.parser,
            &harness.output_rx,
            LIVE_STATE_FIXTURES.permission.marker,
            MARKER_TIMEOUT,
        )
        .expect("wait for permission marker before narrow post-run handoff");
        LIVE_STATE_FIXTURES
            .permission
            .approve(harness.writer.as_mut())
            .expect("approve permission before narrow post-run handoff");

        let post_run_screen = wait_for_screen_contains(
            &mut harness.parser,
            &harness.output_rx,
            NEXT_ACTION_MARKER,
            MARKER_TIMEOUT,
        )
        .expect("wait for narrow post-run handoff marker");
        let post_run_visual = capture_visual_checkpoint(
            artifact_name,
            &harness.parser,
            &visual_dir,
            FocusCapture::anchored_exact(NEXT_ACTION_MARKER, 28, 6),
        )
        .expect("capture narrow post-run handoff image");

        insta::assert_snapshot!(
            snapshot_name,
            checkpoint_visual_snapshot(
                &post_run_screen,
                POST_RUN_HANDOFF_MARKERS,
                &post_run_visual
            )
        );

        harness
            .child
            .kill()
            .expect("terminate narrow post-run handoff after checkpoint");
        std::mem::forget(harness.child);
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
    let active_visual = capture_visual_checkpoint(
        "continue_rejected_active",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("tasks are still in flight", 28, 1),
    )
    .expect("capture active-session rejection image");
    insta::assert_snapshot!(
        "pty_continue_rejected_active",
        checkpoint_visual_snapshot(
            &active_screen,
            &[
                STARTUP_CONTINUE_HISTORY_MARKER,
                "continue blocked",
                "tasks are still in flight",
            ],
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
    let unrestorable_visual = capture_visual_checkpoint(
        "continue_rejected_unrestorable",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("events unavailable", 28, 1),
    )
    .expect("capture unrestorable-session rejection image");
    insta::assert_snapshot!(
        "pty_continue_rejected_unrestorable",
        checkpoint_visual_snapshot(
            &unrestorable_screen,
            &[
                STARTUP_CONTINUE_HISTORY_MARKER,
                "continue blocked",
                "events unavailable",
            ],
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
fn pty_e2e_replay_never_appends_events() {
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
    let replay_visual = capture_visual_checkpoint(
        "replay_read_only",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(REPLAY_READY_MARKER, 20, 2),
    )
    .expect("capture replay read-only image");
    insta::assert_snapshot!(
        "pty_replay_read_only",
        checkpoint_visual_snapshot(
            &replay_screen,
            &[
                REPLAY_READY_MARKER,
                "run_replay_safe",
                "Replay archive · read-only",
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
    assert_eq!(permission_bytes.first().copied(), Some(b'a'));
    assert_eq!(permission_bytes.get(1).copied(), Some(b' '));
    assert_eq!(
        permission_bytes.len(),
        2 + LIVE_STATE_FIXTURES.permission.rewind_steps
    );
}

fn write_wiremock_tui_config(session_dir: &Path, wiremock_uri: &str) -> PathBuf {
    let config_path = session_dir.join("wiremock-tui-config.jsonc");
    let body = format!(
        r#"{{
  backgroundTask: {{
    defaultConcurrency: 2,
    providerConcurrency: 2,
    modelConcurrency: 2,
    staleTimeoutMs: 15000,
    messageStalenessTimeoutMs: 5000,
  }},
  providers: {{
    default: {{
      type: "openai_compatible",
      base_url: "{wiremock_uri}/v1",
      api_key: "test-key",
      api_mode: "responses",
      models: {{
        "model-1": {{
          display_name: "Model 1",
        }},
      }},
    }},
  }},
  categories: {{
    deep: {{
      description: "deep work agent",
      model_ref: "default:model-1",
      tools: ["fs.read"],
    }},
    worker: {{
      description: "worker agent",
      model_ref: "default:model-1",
      tools: ["fs.read"],
    }},
  }},
  permissions: {{
    edit: "ask",
    shell: "deny",
    network: "deny",
  }},
  paths: {{
    session_dir: "{}",
  }},
  deterministic: {{
    enabled: true,
    seed: 42,
  }},
  ui: {{
    defaultProfile: "worker",
  }},
}}"#,
        session_dir.display()
    );
    fs::write(&config_path, body).expect("write temporary wiremock TUI config");
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
            contract.snapshot.starts_with("pty_"),
            "offline evidence snapshot must stay under the pty_* naming contract: {contract:?}"
        );
        assert!(
            unique_pngs.insert(contract.png),
            "offline evidence PNG names must stay unique: {}",
            contract.png
        );
        assert!(
            unique_snapshots.insert(contract.snapshot),
            "offline evidence snapshot names must stay unique: {}",
            contract.snapshot
        );
        covered_family_states.insert((contract.family, contract.state));
    }

    for required in [
        ("startup", "happy_path"),
        ("startup", "responsive"),
        ("continue_session", "happy_path"),
        ("continue_session", "failure_path"),
        ("permission", "happy_path"),
        ("live_shell", "happy_path"),
        ("live_details", "happy_path"),
        ("live_details", "responsive"),
        ("live_secondary", "happy_path"),
        ("post_run", "happy_path"),
        ("post_run", "responsive"),
        ("replay", "happy_path"),
        ("replay", "failure_path"),
        ("replay_secondary", "happy_path"),
        ("replay_secondary", "responsive"),
    ] {
        assert!(
            covered_family_states.contains(&required),
            "offline PTY corpus must cover {:?}",
            required
        );
    }
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

fn drain_output(parser: &mut vt100::Parser, output_rx: &Receiver<Vec<u8>>) {
    while let Ok(chunk) = output_rx.try_recv() {
        parser.process(&chunk);
    }
}

fn screen_contents(parser: &vt100::Parser) -> String {
    parser.screen().contents()
}

type AntiAliasMask = Arc<Vec<u8>>;
type AntiAliasCache = RefCell<BTreeMap<(char, u32), AntiAliasMask>>;

#[derive(Debug)]
struct VisualCheckpoint {
    file_name: String,
    focus_pixels_blake3: String,
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
    lines.push(format!(
        "focus_pixels_blake3: {}",
        visual.focus_pixels_blake3
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
        "size_px: {}x{}",
        visual.size_px.0, visual.size_px.1
    ));
    lines.join("\n")
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
    let image = render_parser_to_image(parser);
    let file_name = format!("pty_{name}.png");
    let path = visual_dir.join(&file_name);
    image
        .save(&path)
        .map_err(|err| format!("failed to save {}: {err}", path.display()))?;

    let (rows, cols) = parser.screen().size();
    let focus_region = find_marker_cell(parser.screen(), focus.marker)
        .map(|(row, col)| anchored_region((row, col), (rows, cols), focus))
        .unwrap_or((0, 0, rows.max(1), cols.max(1)));

    let focus_pixels = extract_region_pixels(&image, focus_region);

    Ok(VisualCheckpoint {
        file_name,
        focus_pixels_blake3: blake3::hash(&focus_pixels).to_hex().to_string(),
        focus_marker: focus.marker.to_string(),
        focus_region_cells: focus_region,
        size_px: (image.width(), image.height()),
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

fn extract_region_pixels(image: &RgbImage, region: (u16, u16, u16, u16)) -> Vec<u8> {
    let (row_start, col_start, height_cells, width_cells) = region;

    let x_start = u32::from(col_start) * CELL_WIDTH;
    let y_start = u32::from(row_start) * CELL_HEIGHT;
    let width_px = u32::from(width_cells) * CELL_WIDTH;
    let height_px = u32::from(height_cells) * CELL_HEIGHT;

    let mut data = Vec::with_capacity((width_px * height_px * 3) as usize);
    for y in y_start..(y_start + height_px) {
        for x in x_start..(x_start + width_px) {
            let [r, g, b] = image.get_pixel(x, y).0;
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }

    data
}

fn render_parser_to_image(parser: &VtParser) -> RgbImage {
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let mut raster_image = RgbImage::new(
        u32::from(cols) * RASTER_CELL_WIDTH,
        u32::from(rows) * RASTER_CELL_HEIGHT,
    );
    let glyphs = GlyphLookup::new();
    let cursor_position = None;

    for row in 0..rows {
        for col in 0..cols {
            draw_cell(
                &mut raster_image,
                screen,
                row,
                col,
                &glyphs,
                cursor_position,
                RASTER_SCALE,
            );
        }
    }

    let target_width = u32::from(cols) * CELL_WIDTH;
    let target_height = u32::from(rows) * CELL_HEIGHT;
    if raster_image.width() == target_width && raster_image.height() == target_height {
        raster_image
    } else {
        image::imageops::resize(
            &raster_image,
            target_width,
            target_height,
            FilterType::Lanczos3,
        )
    }
}

fn draw_cell(
    image: &mut RgbImage,
    screen: &vt100::Screen,
    row: u16,
    col: u16,
    glyphs: &GlyphLookup,
    cursor_position: Option<(u16, u16)>,
    raster_scale: u32,
) {
    let cell_width = GLYPH_WIDTH * raster_scale;
    let cell_height = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * raster_scale;
    let origin_x = u32::from(col) * cell_width;
    let origin_y = u32::from(row) * cell_height;
    let Some(cell) = screen.cell(row, col) else {
        fill_cell_background(
            image,
            origin_x,
            origin_y,
            DEFAULT_BG,
            cell_width,
            cell_height,
        );
        return;
    };

    let cursor_over_cell = cursor_position == Some((row, col));
    let mut fg = terminal_color_to_rgb(cell.fgcolor(), true);
    let mut bg = terminal_color_to_rgb(cell.bgcolor(), false);
    if cell.inverse() && !cursor_over_cell {
        std::mem::swap(&mut fg, &mut bg);
    }
    if cell.bold() {
        fg = brighten(fg, 28);
    }

    fill_cell_background(image, origin_x, origin_y, bg, cell_width, cell_height);
    if cell.is_wide_continuation() {
        return;
    }

    let Some(ch) = cell.contents().chars().next() else {
        return;
    };

    draw_cell_glyph(image, origin_x, origin_y, ch, fg, glyphs, raster_scale);
    if cell.underline() {
        draw_underline(
            image,
            origin_x,
            origin_y,
            fg,
            raster_scale,
            cell_width,
            cell_height,
        );
    }
}

fn fill_cell_background(
    image: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    color: [u8; 3],
    cell_width: u32,
    cell_height: u32,
) {
    for y in 0..cell_height {
        for x in 0..cell_width {
            image.put_pixel(origin_x + x, origin_y + y, Rgb(color));
        }
    }
}

fn draw_cell_glyph(
    image: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    ch: char,
    color: [u8; 3],
    glyphs: &GlyphLookup,
    raster_scale: u32,
) {
    if ch == ' ' {
        return;
    }

    if ttf_antialias_enabled()
        && !prefers_bitmap_terminal_glyph(ch)
        && glyphs.draw_antialiased_glyph(image, origin_x, origin_y, ch, color, raster_scale)
    {
        return;
    }

    let glyph = glyphs
        .glyph(ch)
        .or_else(|| glyphs.glyph('?'))
        .unwrap_or([0_u8; 8]);

    for (glyph_row, row_bits) in glyph.into_iter().enumerate() {
        let glyph_row = u32::try_from(glyph_row).expect("glyph row in u32");
        for bit in 0_u8..8 {
            let pixel_is_on = row_bits & (1_u8 << bit) != 0;
            if !pixel_is_on {
                continue;
            }

            let pixel_x = origin_x + u32::from(bit) * raster_scale;
            let pixel_y = origin_y + glyph_row * GLYPH_VERTICAL_SCALE * raster_scale;
            for y in 0..(GLYPH_VERTICAL_SCALE * raster_scale) {
                for x in 0..raster_scale {
                    image.put_pixel(pixel_x + x, pixel_y + y, Rgb(color));
                }
            }
        }
    }
}

fn prefers_bitmap_terminal_glyph(ch: char) -> bool {
    let code = ch as u32;
    (0x2500..=0x259F).contains(&code)
}

fn draw_underline(
    image: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    color: [u8; 3],
    raster_scale: u32,
    cell_width: u32,
    cell_height: u32,
) {
    let thickness = raster_scale.max(1);
    let y_start = origin_y + cell_height.saturating_sub(thickness);
    for y in 0..thickness {
        for x in 0..cell_width {
            image.put_pixel(origin_x + x, y_start + y, Rgb(color));
        }
    }
}

fn terminal_color_to_rgb(color: VtColor, foreground: bool) -> [u8; 3] {
    match color {
        VtColor::Default => {
            if foreground {
                DEFAULT_FG
            } else {
                DEFAULT_BG
            }
        }
        VtColor::Idx(idx) => xterm_256_color(idx),
        VtColor::Rgb(r, g, b) => [r, g, b],
    }
}

fn xterm_256_color(idx: u8) -> [u8; 3] {
    const ANSI_16: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];

    match idx {
        0..=15 => ANSI_16[usize::from(idx)],
        16..=231 => {
            let value = idx - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            [cube_level(r), cube_level(g), cube_level(b)]
        }
        232..=255 => {
            let gray = 8_u8.saturating_add((idx - 232) * 10);
            [gray, gray, gray]
        }
    }
}

fn cube_level(level: u8) -> u8 {
    if level == 0 {
        0
    } else {
        level.saturating_mul(40).saturating_add(55)
    }
}

fn brighten(color: [u8; 3], amount: u8) -> [u8; 3] {
    [
        color[0].saturating_add(amount),
        color[1].saturating_add(amount),
        color[2].saturating_add(amount),
    ]
}

struct GlyphLookup {
    basic: BasicFonts,
    latin: LatinFonts,
    box_drawing: BoxFonts,
    block: BlockFonts,
    smooth_font: Option<Font>,
    anti_alias_cache: AntiAliasCache,
}

impl GlyphLookup {
    fn new() -> Self {
        Self {
            basic: BasicFonts::new(),
            latin: LatinFonts::new(),
            box_drawing: BoxFonts::new(),
            block: BlockFonts::new(),
            smooth_font: load_anti_alias_font(),
            anti_alias_cache: RefCell::new(BTreeMap::new()),
        }
    }

    fn draw_antialiased_glyph(
        &self,
        image: &mut RgbImage,
        origin_x: u32,
        origin_y: u32,
        ch: char,
        color: [u8; 3],
        raster_scale: u32,
    ) -> bool {
        let Some(mask) = self.anti_alias_mask_for(ch, raster_scale) else {
            return false;
        };

        let cell_width = GLYPH_WIDTH * raster_scale;
        let cell_height = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * raster_scale;
        for y in 0..cell_height {
            for x in 0..cell_width {
                let idx = usize::try_from(y * cell_width + x).expect("mask index in usize");
                let alpha = mask[idx];
                if alpha == 0 {
                    continue;
                }

                let pixel = image.get_pixel_mut(origin_x + x, origin_y + y);
                let [r, g, b] = pixel.0;
                let a = u16::from(alpha);
                let inv = 255_u16.saturating_sub(a);
                pixel.0 = [
                    ((u16::from(r) * inv + u16::from(color[0]) * a + 127) / 255) as u8,
                    ((u16::from(g) * inv + u16::from(color[1]) * a + 127) / 255) as u8,
                    ((u16::from(b) * inv + u16::from(color[2]) * a + 127) / 255) as u8,
                ];
            }
        }

        true
    }

    fn anti_alias_mask_for(&self, ch: char, raster_scale: u32) -> Option<AntiAliasMask> {
        let cache_key = (ch, raster_scale);
        if let Some(mask) = self.anti_alias_cache.borrow().get(&cache_key) {
            return Some(mask.clone());
        }

        let font = self.smooth_font.as_ref()?;
        let cell_width = GLYPH_WIDTH * raster_scale;
        let cell_height = GLYPH_HEIGHT * GLYPH_VERTICAL_SCALE * raster_scale;
        let font_size = (cell_height as f32 * ANTI_ALIAS_FONT_SIZE_FACTOR).max(8.0);
        let line_metrics = font.horizontal_line_metrics(font_size);
        let reference_metrics = font.metrics('M', font_size);
        let baseline_origin_x =
            ((cell_width as f32 - reference_metrics.advance_width).max(0.0) * 0.5).round() as i32;
        let baseline_y = line_metrics
            .map(|line| {
                let centered_top =
                    ((cell_height as f32 - line.new_line_size).max(0.0) * 0.5).round();
                (centered_top + line.ascent).round() as i32
            })
            .unwrap_or_else(|| {
                i32::try_from(cell_height / 2).expect("cell height midpoint in i32")
            });

        let (metrics, bitmap) = font.rasterize(ch, font_size);
        let mut mask = vec![0_u8; (cell_width * cell_height) as usize];
        if metrics.width == 0 || metrics.height == 0 {
            let mask = Arc::new(mask);
            self.anti_alias_cache
                .borrow_mut()
                .insert(cache_key, mask.clone());
            return Some(mask);
        }

        let glyph_width = u32::try_from(metrics.width).expect("glyph width fits u32");
        let glyph_height = u32::try_from(metrics.height).expect("glyph height fits u32");
        let x_offset = baseline_origin_x + metrics.xmin;
        let y_offset =
            baseline_y - metrics.ymin - i32::try_from(glyph_height).expect("glyph height in i32");

        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let src_idx = usize::try_from(y * glyph_width + x).expect("bitmap index in usize");
                let alpha = bitmap[src_idx];
                if alpha == 0 {
                    continue;
                }

                let dx = x_offset + i32::try_from(x).expect("x fits i32");
                let dy = y_offset + i32::try_from(y).expect("y fits i32");
                if dx < 0 || dy < 0 {
                    continue;
                }

                let dx = u32::try_from(dx).expect("dx non-negative");
                let dy = u32::try_from(dy).expect("dy non-negative");
                if dx >= cell_width || dy >= cell_height {
                    continue;
                }

                let dst_idx = usize::try_from(dy * cell_width + dx).expect("mask index in usize");
                mask[dst_idx] = alpha;
            }
        }

        let mask = Arc::new(mask);
        self.anti_alias_cache
            .borrow_mut()
            .insert(cache_key, mask.clone());
        Some(mask)
    }

    fn glyph(&self, ch: char) -> Option<[u8; 8]> {
        self.basic
            .get(ch)
            .or_else(|| self.latin.get(ch))
            .or_else(|| self.box_drawing.get(ch))
            .or_else(|| self.block.get(ch))
    }
}

fn load_anti_alias_font() -> Option<Font> {
    if !ttf_antialias_enabled() {
        return None;
    }

    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("HARNESS_VISUAL_FONT_PATH") {
        candidates.push(path);
    }
    candidates.push("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf".to_string());
    candidates.push("/usr/share/fonts/dejavu/DejaVuSansMono.ttf".to_string());

    for path in candidates {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) else {
            continue;
        };
        return Some(font);
    }

    None
}

fn ttf_antialias_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("HARNESS_VISUAL_TTF_ANTIALIAS")
            .map(|value| {
                let normalized = value.trim();
                !(normalized == "0"
                    || normalized.eq_ignore_ascii_case("false")
                    || normalized.eq_ignore_ascii_case("off"))
            })
            .unwrap_or(true)
    })
}

fn visual_artifacts_dir() -> PathBuf {
    static PREPARED: OnceLock<()> = OnceLock::new();
    let dir = if let Ok(dir) = std::env::var("HARNESS_VISUAL_ARTIFACT_DIR") {
        resolve_artifact_root(dir)
    } else {
        repo_root().join("target").join("pty-visual-artifacts")
    };

    PREPARED.get_or_init(|| {
        fs::create_dir_all(&dir).expect("create visual artifacts dir");
        prune_stale_pty_visual_artifacts(&dir).expect("prune stale PTY visual artifacts");
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
    let base = std::env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp session dir");

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let dir = base.join(format!("pty-e2e-{}-{suffix}", std::process::id()));
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
            session_event(
                run_id,
                7,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "segment complete",
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
                    "queue_key": "tool:shell.run",
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
                "run_finished",
                json!({
                    "summary": "done",
                }),
            ),
        ],
    )
}

fn write_replay_diff_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
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
                event_actor("user", Some("interactive-user")),
                Some("req_replay_diff"),
                "user_message_submitted",
                json!({
                    "request_id": "req_replay_diff",
                    "text": "Show the generated diff",
                }),
            ),
            session_event(
                run_id,
                3,
                event_actor("worker", Some("agent_000001")),
                Some("req_replay_diff"),
                "edit_applied",
                json!({
                    "edit_id": "edit_replay_diff",
                    "path": "demo.txt",
                    "new_file_digest": "digest-replay-diff-file",
                    "diff_rel_path": "artifacts/edit-replay-diff.diff",
                    "diff_digest": "digest-replay-diff-artifact",
                }),
            ),
            session_event(
                run_id,
                4,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "diff ready",
                }),
            ),
        ],
    )
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
    json!({
        "schema_version": 1,
        "event_id": format!("evt-{seq:04}"),
        "seq": seq,
        "run_id": run_id,
        "mono_ms": seq,
        "ts": Value::Null,
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
