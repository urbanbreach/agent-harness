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

const VISUAL_MANIFEST_JSON_FILE: &str = "manifest.json";
const VISUAL_MANIFEST_JSONL_FILE: &str = "manifest.jsonl";

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
const STARTUP_HOME_WORDMARK_MARKER: &str = "Harness";
const STARTUP_HOME_ASCII_WORDMARK_MARKER: &str = "HARNESS";
const STARTUP_HOME_SHORTCUT_MARKER: &str = "ctrl+p commands";
const STARTUP_HOME_VALUE_PROP_MARKER: &str =
    "Dispatch a new run, reopen live work, or inspect saved history.";
const STARTUP_HOME_DENSE_VALUE_PROP_MARKER: &str =
    "Dispatch a new run, reopen live work, or inspect saved";
const STARTUP_LAUNCHER_READY_MARKER: &str = STARTUP_HOME_SHORTCUT_MARKER;
const STARTUP_COMMAND_PALETTE_MARKER: &str = "Command palette";
const STARTUP_CONTINUE_HISTORY_MARKER: &str = "Continue session";
const STARTUP_REPLAY_HISTORY_MARKER: &str = "Replay session";
const STARTUP_CONTINUE_HISTORY_READY_MARKER: &str = "continue ready";
const REPLAY_READY_MARKER: &str = "q quit";
const REPLAY_DENSE_READY_MARKER: &str = "Replay · read-only";
const CONTINUED_SESSION_TITLE_MARKER: &str = "Continued · run";
const STRUCTURED_DIFF_FILE_MARKER: &str = "demo.txt · +1 -1";
const STRUCTURED_DIFF_REMOVED_LINE_MARKER: &str = "- beta";
const STRUCTURED_DIFF_ADDED_LINE_MARKER: &str = "+ BETA";
const RUN_FINISHED_SHELL_MARKERS: &[&str] = &[
    "run finished",
    "session shell preserved",
    "Run complete",
    "Tab focus",
];

#[test]
fn pty_e2e_permission_overlay_parity() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let mut harness = spawn_harness_tui(PtyGeometry::MINIMUM_SIGNOFF, &session_dir, |command| {
        command.arg("--scenario");
        command.arg("golden_path_interactive");
    });

    LIVE_STATE_FIXTURES
        .permission
        .write_preserved_draft(harness.writer.as_mut())
        .expect("queue preserved draft before permission prompt");

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
        "permission_overlay_parity",
        &harness.parser,
        &visual_dir,
        LIVE_STATE_FIXTURES.permission.focus_capture,
        &[
            LIVE_STATE_FIXTURES.permission.marker,
            "FAIL CLOSED",
            "SESSION PAUSED",
            LIVE_STATE_FIXTURES.permission.draft_text,
        ],
    )
    .expect("capture permission parity checkpoint image");

    assert!(stable_screen.contains(LIVE_STATE_FIXTURES.permission.marker));
    assert!(stable_screen.contains("FAIL CLOSED"));
    assert!(stable_screen.contains("SESSION PAUSED"));
    assert!(stable_screen.contains(LIVE_STATE_FIXTURES.permission.draft_text));
    assert!(!stable_screen.contains("Permission blocked"));
    assert!(!stable_screen.contains(STARTUP_COMMAND_PALETTE_MARKER));
    assert!(visual_dir.join(&permission_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_permission_overlay_parity",
        checkpoint_visual_snapshot(
            &stable_screen,
            &[
                LIVE_STATE_FIXTURES.permission.marker,
                "FAIL CLOSED",
                "SESSION PAUSED",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct OfflineVisualEvidenceContract {
    family: &'static str,
    state: &'static str,
    png: &'static str,
    snapshot: &'static str,
}

const OFFLINE_VISUAL_EVIDENCE_CONTRACTS: &[OfflineVisualEvidenceContract] = &[
    OfflineVisualEvidenceContract {
        family: "startup_shell",
        state: "happy_path",
        png: "pty_startup_home_primary.png",
        snapshot: "pty_startup_home_primary",
    },
    OfflineVisualEvidenceContract {
        family: "startup_shell",
        state: "happy_path",
        png: "pty_startup_home_dense.png",
        snapshot: "pty_startup_home_dense",
    },
    OfflineVisualEvidenceContract {
        family: "startup_command_palette",
        state: "happy_path",
        png: "pty_startup_command_palette.png",
        snapshot: "pty_startup_command_palette",
    },
    OfflineVisualEvidenceContract {
        family: "startup_session_history",
        state: "continue_history",
        png: "pty_startup_continue_history.png",
        snapshot: "pty_startup_continue_history",
    },
    OfflineVisualEvidenceContract {
        family: "startup_session_history",
        state: "replay_history",
        png: "pty_startup_replay_history.png",
        snapshot: "pty_startup_replay_history",
    },
    OfflineVisualEvidenceContract {
        family: "continue_session",
        state: "happy_path",
        png: "pty_continue_quiescent_session.png",
        snapshot: "pty_continue_quiescent_session",
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
        png: "pty_permission_overlay_parity.png",
        snapshot: "pty_permission_overlay_parity",
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
        family: "live_shell",
        state: "happy_path",
        png: "pty_session_shell_primary_live.png",
        snapshot: "pty_session_shell_primary_live",
    },
    OfflineVisualEvidenceContract {
        family: "live_shell",
        state: "inline_completion",
        png: "pty_inline_completion_shell.png",
        snapshot: "pty_inline_completion_shell",
    },
    OfflineVisualEvidenceContract {
        family: "replay_shell",
        state: "happy_path",
        png: "pty_session_shell_primary_replay.png",
        snapshot: "pty_session_shell_primary_replay",
    },
    OfflineVisualEvidenceContract {
        family: "transcript_shell",
        state: "happy_path",
        png: "pty_session_transcript_rich_shell.png",
        snapshot: "pty_session_transcript_rich_shell",
    },
    OfflineVisualEvidenceContract {
        family: "operator_sidebar",
        state: "happy_path",
        png: "pty_operator_sidebar_primary.png",
        snapshot: "pty_operator_sidebar_primary",
    },
    OfflineVisualEvidenceContract {
        family: "diff_surface",
        state: "live_secondary",
        png: "pty_continue_live_diff_secondary.png",
        snapshot: "pty_continue_live_diff_secondary",
    },
    OfflineVisualEvidenceContract {
        family: "diff_surface",
        state: "replay_narrow",
        png: "pty_replay_diff_tab.png",
        snapshot: "pty_replay_diff_tab",
    },
    OfflineVisualEvidenceContract {
        family: "diff_surface",
        state: "replay_dense",
        png: "pty_replay_diff_six_window.png",
        snapshot: "pty_replay_diff_six_window",
    },
    OfflineVisualEvidenceContract {
        family: "replay",
        state: "failure_path",
        png: "pty_replay_read_only.png",
        snapshot: "pty_replay_read_only",
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
        ready_marker: STARTUP_HOME_SHORTCUT_MARKER,
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
        snapshot_markers: &["Hello world", "ready for next turn"],
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
        approve_key: 0x19,
        follow_toggle_key: b' ',
        rewind_key: b'k',
        rewind_steps: 64,
        focus_capture: FocusCapture::anchored_exact("Permission Requested", 24, 1),
        snapshot_markers: &[
            "Permission Requested",
            "FAIL CLOSED",
            "keep this draft",
            "Ctrl+n deny",
            "Draft preserved",
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

fn show_live_operator_sidebar(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
) -> String {
    send_key(writer, b'\t').expect("focus transcript before opening operator sidebar");
    send_key(writer, b'i').expect("toggle live operator sidebar");
    wait_for_screen_contains(parser, output_rx, "Context", MARKER_TIMEOUT)
        .expect("wait for live operator sidebar")
}

fn open_secondary_review_surface(
    parser: &mut VtParser,
    output_rx: &Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    command_filter: &str,
    marker: &str,
) -> String {
    send_key(writer, b'\t').expect("move focus off prompt before review command");
    send_key(writer, 0x10).expect("open command palette for review surface");
    writer
        .write_all(command_filter.as_bytes())
        .expect("filter review surface command");
    writer.flush().expect("flush review surface command");
    send_key(writer, b'\r').expect("open review surface from command palette");
    let current = wait_for_screen_contains(parser, output_rx, marker, MARKER_TIMEOUT)
        .expect("wait for secondary review surface");
    stabilize_screen(parser, output_rx, current)
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
        "ready for next turn",
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
    let visual = capture_manifest_backed_visual_checkpoint(
        "startup_shell",
        "startup_home_primary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_HOME_WORDMARK_MARKER, 32, 8),
        &[
            STARTUP_HOME_WORDMARK_MARKER,
            STARTUP_HOME_SHORTCUT_MARKER,
            STARTUP_HOME_VALUE_PROP_MARKER,
            "/status · v",
        ],
    )
    .expect("capture startup home primary image");

    assert!(screen.contains(STARTUP_HOME_WORDMARK_MARKER));
    assert!(screen.contains(STARTUP_HOME_SHORTCUT_MARKER));
    assert!(screen.contains("enter send"));
    assert!(screen.contains(STARTUP_HOME_VALUE_PROP_MARKER));
    assert!(screen.contains("/status · v"));
    assert!(!screen.contains("New session"));
    assert!(!screen.contains("Continue session"));
    assert!(!screen.contains("Replay session"));
    assert!(visual_dir.join(&visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_home_primary",
        checkpoint_visual_snapshot(
            &screen,
            &[
                STARTUP_HOME_WORDMARK_MARKER,
                STARTUP_HOME_SHORTCUT_MARKER,
                STARTUP_HOME_VALUE_PROP_MARKER,
                "/status · v",
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
        STARTUP_HOME_SHORTCUT_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for dense compose-first startup home shell");
    let visual = capture_manifest_backed_visual_checkpoint(
        "startup_shell",
        "startup_home_dense",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_HOME_ASCII_WORDMARK_MARKER, 24, 8),
        &[
            STARTUP_HOME_ASCII_WORDMARK_MARKER,
            STARTUP_HOME_SHORTCUT_MARKER,
            STARTUP_HOME_DENSE_VALUE_PROP_MARKER,
            "/status",
        ],
    )
    .expect("capture startup home dense image");

    assert!(screen.contains(STARTUP_HOME_ASCII_WORDMARK_MARKER));
    assert!(screen.contains(STARTUP_HOME_SHORTCUT_MARKER));
    assert!(screen.contains("enter send"));
    assert!(screen.contains(STARTUP_HOME_DENSE_VALUE_PROP_MARKER));
    assert!(screen.contains("/status"));
    assert!(!screen.contains("New session"));
    assert!(!screen.contains("Continue session"));
    assert!(!screen.contains("Replay session"));
    assert!(visual_dir.join(&visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_startup_home_dense",
        checkpoint_visual_snapshot(
            &screen,
            &[
                STARTUP_HOME_ASCII_WORDMARK_MARKER,
                STARTUP_HOME_SHORTCUT_MARKER,
                STARTUP_HOME_DENSE_VALUE_PROP_MARKER,
                "/status",
            ],
            &visual,
        )
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
        "Context",
        STARTUP_TIMEOUT,
    )
    .expect("wait for wide live session shell sidebar");
    let live_visual = capture_manifest_backed_visual_checkpoint(
        "live_shell",
        "session_shell_primary_live",
        &live.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Context", 20, 8),
        &[
            "Context",
            "Modified Files",
            "shell parity task",
            "Enter send",
        ],
    )
    .expect("capture primary live session shell image");

    assert!(live_screen.contains("Context"));
    assert!(live_screen.contains("Modified Files"));
    assert!(live_screen.contains("Enter send"));
    assert!(!live_screen.contains("Tabs"));
    assert!(visual_dir.join(&live_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_session_shell_primary_live",
        checkpoint_visual_snapshot(
            &live_screen,
            &[
                "Context",
                "Modified Files",
                "shell parity task",
                "Enter send"
            ],
            &live_visual,
        )
    );

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
            "Context",
            "Modified Files",
            REPLAY_READY_MARKER,
        ],
    )
    .expect("capture primary replay session shell image");

    assert!(replay_screen.contains(REPLAY_DENSE_READY_MARKER));
    assert!(replay_screen.contains("Context"));
    assert!(replay_screen.contains("Modified Files"));
    assert!(!replay_screen.contains("Tabs"));
    assert!(visual_dir.join(&replay_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_session_shell_primary_replay",
        checkpoint_visual_snapshot(
            &replay_screen,
            &[
                REPLAY_DENSE_READY_MARKER,
                "Context",
                "Modified Files",
                REPLAY_READY_MARKER,
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

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_session_transcript_rich_shell() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;
    let session_dir = create_temp_session_dir();
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");
    let run_id = "run_rich_transcript";
    write_rich_transcript_fixture(&session_dir, run_id);
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());

    let mut harness = spawn_harness_tui(PtyGeometry::PRIMARY_SIGNOFF, &session_dir, |command| {
        command.arg("--config");
        command.arg(config_path.to_string_lossy().to_string());
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
        .expect("select rich transcript replay session from history");

    let screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for rich transcript replay shell");
    let visual = capture_manifest_backed_visual_checkpoint(
        "transcript_shell",
        "session_transcript_rich_shell",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Thinking:", 30, 8),
        &[
            "Restyle the transcript shell",
            "Drafting a document-like plan",
            "tool fs.read (succeeded) · 24 lines read from src/ui.rs",
            "Found the transcript renderer and the composer chrome.",
        ],
    )
    .expect("capture rich transcript shell image");

    assert!(screen.contains("Restyle the transcript shell"));
    assert!(screen.contains("Drafting a document-like plan"));
    assert!(screen.contains("tool fs.read (succeeded) · 24 lines read from src/ui.rs"));
    assert!(screen.contains("Found the transcript renderer and the composer chrome."));
    assert!(screen.contains(REPLAY_DENSE_READY_MARKER));
    assert!(!screen.contains("╭─"));
    assert!(!screen.contains("╰─"));
    assert!(visual_dir.join(&visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_session_transcript_rich_shell",
        checkpoint_visual_snapshot(
            &screen,
            &[
                "Restyle the transcript shell",
                "Drafting a document-like plan",
                "tool fs.read (succeeded) · 24 lines read from src/ui.rs",
                "Found the transcript renderer and the composer chrome.",
            ],
            &visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate rich transcript session harness");
    std::mem::forget(harness.child);
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
    assert!(visual_dir.join(&permission_visual.file_name).exists());

    LIVE_STATE_FIXTURES
        .permission
        .approve(scenario_harness.writer.as_mut())
        .expect("send approve key");

    let inline_completion_checkpoint = wait_for_screen_contains(
        &mut scenario_harness.parser,
        &scenario_harness.output_rx,
        "run finished",
        MARKER_TIMEOUT,
    )
    .expect("wait for run-finished shell marker");
    let inline_completion_visual = capture_manifest_backed_visual_checkpoint(
        "live_shell",
        "inline_completion_shell",
        &scenario_harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("run finished", 20, 1),
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
        FocusCapture::anchored_exact(STARTUP_COMMAND_PALETTE_MARKER, 28, 5),
        &[
            STARTUP_COMMAND_PALETTE_MARKER,
            "New session",
            STARTUP_CONTINUE_HISTORY_MARKER,
            STARTUP_REPLAY_HISTORY_MARKER,
        ],
    )
    .expect("capture startup command palette image");
    assert!(!palette_screen.contains("enter send"));
    assert!(!palette_screen.contains(STARTUP_HOME_VALUE_PROP_MARKER));
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
    assert!(visual_dir.join(&startup_visual.file_name).exists());

    let replay_history_screen = open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    let replay_history_visual = capture_manifest_backed_visual_checkpoint(
        "startup_session_history",
        "startup_replay_history",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_REPLAY_HISTORY_MARKER, 28, 3),
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
        checkpoint_visual_snapshot(
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

    let history_screen = open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "resume",
        STARTUP_CONTINUE_HISTORY_READY_MARKER,
    );
    let history_visual = capture_manifest_backed_visual_checkpoint(
        "startup_session_history",
        "startup_continue_history",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(STARTUP_CONTINUE_HISTORY_READY_MARKER, 28, 3),
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
        checkpoint_visual_snapshot(
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
        CONTINUED_SESSION_TITLE_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for continued session shell after reopening run");
    let continued_visual = capture_manifest_backed_visual_checkpoint(
        "continue_session",
        "continue_quiescent_session",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact(CONTINUED_SESSION_TITLE_MARKER, 18, 1),
        &[
            &format!("Continued · run {run_id}"),
            "ready for next turn",
            "Context",
            "Enter send",
        ],
    )
    .expect("capture continued session checkpoint image");
    insta::assert_snapshot!(
        "pty_continue_quiescent_session",
        checkpoint_visual_snapshot(
            &continued_screen,
            &[
                &format!("Continued · run {run_id}"),
                "ready for next turn",
                "Context",
                "Enter send",
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

    let sidebar_screen = show_live_operator_sidebar(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
    );
    let sidebar_visual = capture_manifest_backed_visual_checkpoint(
        "operator_sidebar",
        "operator_sidebar_primary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Context", 20, 6),
        &[
            "Context",
            "Context unavailable",
            "No MCPs connected",
            "LSPs will activate as files are read",
            "demo.txt · +1 -1",
        ],
    )
    .expect("capture operator sidebar primary image");

    insta::assert_snapshot!(
        "pty_operator_sidebar_primary",
        checkpoint_visual_snapshot(
            &sidebar_screen,
            &[
                "Context",
                "Context unavailable",
                "No MCPs connected",
                "LSPs will activate as files are read",
                "demo.txt · +1 -1",
            ],
            &sidebar_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate operator sidebar harness");
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
        );
        let details_visual = capture_visual_checkpoint(
            artifact_name,
            &harness.parser,
            &visual_dir,
            FocusCapture::anchored_exact("Context", 20, 6),
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

#[tokio::test(flavor = "current_thread")]
async fn pty_e2e_live_diff_surface_remains_reachable_from_session_shell() {
    if !cfg!(target_os = "linux") {
        return;
    }

    let server = start_responses_wiremock_server().await;
    let visual_dir = visual_artifacts_dir();
    fs::create_dir_all(&visual_dir).expect("create visual artifacts dir");

    let session_dir = create_temp_session_dir();
    let run_id = "run_resume_diff";
    write_quiescent_resume_diff_fixture(&session_dir, run_id);
    let config_path = write_wiremock_tui_config(&session_dir, &server.uri());
    let mut harness = spawn_resumed_quiescent_session(
        PtyGeometry::PRIMARY_SIGNOFF,
        &session_dir,
        &config_path,
        run_id,
    );

    let _diff_screen = open_secondary_review_surface(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "diff",
        "demo.txt · +1 -1",
    );
    let diff_screen = harness.parser.screen().contents();
    let diff_visual = capture_manifest_backed_visual_checkpoint(
        "diff_surface",
        "continue_live_diff_secondary",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored("demo.txt · +1 -1", 28, 8),
        &[
            STRUCTURED_DIFF_FILE_MARKER,
            STRUCTURED_DIFF_REMOVED_LINE_MARKER,
            STRUCTURED_DIFF_ADDED_LINE_MARKER,
        ],
    )
    .expect("capture live diff tab image");

    assert!(diff_screen.contains(STRUCTURED_DIFF_FILE_MARKER));
    assert!(diff_screen.contains(STRUCTURED_DIFF_REMOVED_LINE_MARKER));
    assert!(diff_screen.contains(STRUCTURED_DIFF_ADDED_LINE_MARKER));
    assert!(!diff_screen.contains("Tabs"));
    assert!(!diff_screen.contains("diff artifact missing:"));
    assert!(visual_dir.join(&diff_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_continue_live_diff_secondary",
        checkpoint_visual_snapshot(
            &diff_screen,
            &[
                STRUCTURED_DIFF_FILE_MARKER,
                STRUCTURED_DIFF_REMOVED_LINE_MARKER,
                STRUCTURED_DIFF_ADDED_LINE_MARKER,
            ],
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

    let mut harness = spawn_harness_tui(PtyGeometry::MINIMUM_SIGNOFF, &session_dir, |command| {
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
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell before opening secondary tabs");

    let _diff_screen = open_secondary_review_surface(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "diff",
        "demo.txt · +1 -1",
    );
    let diff_screen = harness.parser.screen().contents();
    let diff_visual = capture_manifest_backed_visual_checkpoint(
        "diff_surface",
        "replay_diff_tab",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored("demo.txt · +1 -1", 24, 8),
        &[
            STRUCTURED_DIFF_FILE_MARKER,
            STRUCTURED_DIFF_REMOVED_LINE_MARKER,
            STRUCTURED_DIFF_ADDED_LINE_MARKER,
            REPLAY_DENSE_READY_MARKER,
        ],
    )
    .expect("capture replay diff tab image");
    assert!(diff_screen.contains(STRUCTURED_DIFF_FILE_MARKER));
    assert!(diff_screen.contains(STRUCTURED_DIFF_REMOVED_LINE_MARKER));
    assert!(diff_screen.contains(STRUCTURED_DIFF_ADDED_LINE_MARKER));
    assert!(!diff_screen.contains("diff artifact missing:"));
    assert!(visual_dir.join(&diff_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_replay_diff_tab",
        checkpoint_visual_snapshot(
            &diff_screen,
            &[
                STRUCTURED_DIFF_FILE_MARKER,
                STRUCTURED_DIFF_REMOVED_LINE_MARKER,
                STRUCTURED_DIFF_ADDED_LINE_MARKER,
                REPLAY_DENSE_READY_MARKER,
            ],
            &diff_visual,
        )
    );

    send_key(harness.writer.as_mut(), b'?').expect("open replay shortcuts review surface");
    let help_screen = wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        "Keyboard Shortcuts:",
        MARKER_TIMEOUT,
    )
    .expect("wait for replay shortcuts surface");
    let help_visual = capture_visual_checkpoint(
        "replay_help_tab",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Keyboard Shortcuts:", 26, 12),
    )
    .expect("capture replay help tab image");
    assert!(!help_screen.contains("Tabs"));
    assert!(visual_dir.join(&help_visual.file_name).exists());

    harness
        .child
        .kill()
        .expect("terminate replay secondary-surface harness");
    std::mem::forget(harness.child);
}

#[test]
fn pty_e2e_replay_diff_covers_dense_secondary_layout() {
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
    .expect("wait for startup launcher before replay dense diff tab");

    open_startup_session_history(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    send_key(harness.writer.as_mut(), b'\r').expect("select replay session for dense diff tab");
    wait_for_screen_contains(
        &mut harness.parser,
        &harness.output_rx,
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell before opening dense diff tab");

    open_secondary_review_surface(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "diff",
        STRUCTURED_DIFF_FILE_MARKER,
    );
    let diff_screen = harness.parser.screen().contents();
    let diff_visual = capture_manifest_backed_visual_checkpoint(
        "diff_surface",
        "replay_diff_six_window",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored(STRUCTURED_DIFF_FILE_MARKER, 18, 6),
        &[
            STRUCTURED_DIFF_FILE_MARKER,
            STRUCTURED_DIFF_REMOVED_LINE_MARKER,
            STRUCTURED_DIFF_ADDED_LINE_MARKER,
            REPLAY_DENSE_READY_MARKER,
        ],
    )
    .expect("capture dense replay diff tab image");

    assert!(diff_screen.contains(STRUCTURED_DIFF_FILE_MARKER));
    assert!(diff_screen.contains(STRUCTURED_DIFF_REMOVED_LINE_MARKER));
    assert!(diff_screen.contains(STRUCTURED_DIFF_ADDED_LINE_MARKER));
    assert!(!diff_screen.contains("diff artifact missing:"));
    assert!(visual_dir.join(&diff_visual.file_name).exists());
    insta::assert_snapshot!(
        "pty_replay_diff_six_window",
        checkpoint_visual_snapshot(
            &diff_screen,
            &[
                STRUCTURED_DIFF_FILE_MARKER,
                STRUCTURED_DIFF_REMOVED_LINE_MARKER,
                STRUCTURED_DIFF_ADDED_LINE_MARKER,
                REPLAY_DENSE_READY_MARKER,
            ],
            &diff_visual,
        )
    );

    harness
        .child
        .kill()
        .expect("terminate dense replay diff harness");
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
        REPLAY_DENSE_READY_MARKER,
        STARTUP_TIMEOUT,
    )
    .expect("wait for replay shell before opening dense events tab");

    let _events_screen = open_secondary_review_surface(
        &mut harness.parser,
        &harness.output_rx,
        harness.writer.as_mut(),
        "event",
        "Event log",
    );
    let events_visual = capture_visual_checkpoint(
        "replay_events_six_window",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("Event log", 20, 6),
    )
    .expect("capture dense replay events tab image");

    assert!(visual_dir.join(&events_visual.file_name).exists());

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

        let _startup_screen = wait_for_screen_contains(
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
        &[
            STARTUP_CONTINUE_HISTORY_MARKER,
            "continue blocked",
            "tasks are still in flight",
        ],
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
    let unrestorable_visual = capture_manifest_backed_visual_checkpoint(
        "continue_session",
        "continue_rejected_unrestorable",
        &harness.parser,
        &visual_dir,
        FocusCapture::anchored_exact("events unavailable", 28, 1),
        &[
            STARTUP_CONTINUE_HISTORY_MARKER,
            "continue blocked",
            "events unavailable",
        ],
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
        ("transcript_shell", "happy_path"),
        ("operator_sidebar", "happy_path"),
        ("diff_surface", "live_secondary"),
        ("diff_surface", "replay_narrow"),
        ("diff_surface", "replay_dense"),
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
    let expected = OFFLINE_VISUAL_EVIDENCE_CONTRACTS
        .iter()
        .map(|contract| format!("pty_e2e__{}.snap", contract.snapshot))
        .collect::<std::collections::BTreeSet<_>>();
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
    manifest_json_path: PathBuf,
    manifest_jsonl_path: PathBuf,
    focus_marker_found: bool,
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
    let focus_marker_found = find_marker_cell(parser.screen(), focus.marker).is_some();

    let focus_pixels = extract_region_pixels(&image, focus_region);

    Ok(VisualCheckpoint {
        file_name,
        manifest_json_path: PathBuf::new(),
        manifest_jsonl_path: PathBuf::new(),
        focus_marker_found,
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
                normalized == "1"
                    || normalized.eq_ignore_ascii_case("true")
                    || normalized.eq_ignore_ascii_case("on")
            })
            .unwrap_or(false)
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

fn write_rich_transcript_fixture(session_dir: &Path, run_id: &str) -> PathBuf {
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
                Some("req_rich_shell"),
                "user_message_submitted",
                json!({
                    "request_id": "req_rich_shell",
                    "text": "Restyle the transcript shell",
                }),
            ),
            session_event(
                run_id,
                4,
                event_actor("worker", Some("agent_000001")),
                Some("req_rich_shell"),
                "provider_request_started",
                json!({
                    "request_id": "req_rich_shell",
                    "provider_id": "default",
                    "model_id": "model-1",
                    "prompt_summary": "Restyle the transcript shell",
                    "request_digest": "digest-rich-shell-request",
                }),
            ),
            session_event(
                run_id,
                5,
                event_actor("worker", Some("agent_000001")),
                Some("req_rich_shell"),
                "provider_stream_delta",
                json!({
                    "request_id": "req_rich_shell",
                    "delta": "Drafting a document-like plan",
                }),
            ),
            session_event(
                run_id,
                6,
                event_actor("system", Some("coordinator")),
                Some("req_rich_shell"),
                "tool_call_requested",
                json!({
                    "tool_call_id": "tc_rich_read",
                    "tool_id": "fs.read",
                    "args_summary": "{\"path\":\"src/ui.rs\",\"start_line\":1,\"limit\":24}",
                    "args_digest": "digest-rich-shell-args",
                }),
            ),
            session_event(
                run_id,
                7,
                event_actor("system", Some("coordinator")),
                Some("req_rich_shell"),
                "tool_call_started",
                json!({
                    "tool_call_id": "tc_rich_read",
                }),
            ),
            session_event(
                run_id,
                8,
                event_actor("system", Some("coordinator")),
                Some("req_rich_shell"),
                "tool_call_finished",
                json!({
                    "tool_call_id": "tc_rich_read",
                    "status": "succeeded",
                    "output_summary": "24 lines read from src/ui.rs",
                    "output_digest": "digest-rich-shell-output",
                }),
            ),
            session_event(
                run_id,
                9,
                event_actor("worker", Some("agent_000001")),
                Some("req_rich_shell"),
                "provider_stream_delta",
                json!({
                    "request_id": "req_rich_shell",
                    "delta": "Found the transcript renderer and the composer chrome.",
                }),
            ),
            session_event(
                run_id,
                10,
                event_actor("system", Some("coordinator")),
                Some("req_rich_shell"),
                "task_completed",
                json!({
                    "task_id": "task_rich_shell",
                    "result_summary": "Found the transcript renderer and the composer chrome.",
                    "result_digest": "digest-rich-shell-result",
                }),
            ),
            session_event(
                run_id,
                11,
                event_actor("system", Some("coordinator")),
                None,
                "run_finished",
                json!({
                    "summary": "rich transcript captured",
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
    );
    write_diff_artifact(
        &run_dir,
        "artifacts/edit-replay-diff.diff",
        sample_diff_artifact(),
    );
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
