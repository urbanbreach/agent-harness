use serde_json::{json, Value};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[path = "support/live_visual.rs"]
#[allow(dead_code)]
mod live_visual;
#[path = "support/markers.rs"]
#[allow(dead_code)]
mod markers;
#[path = "support/native_visual.rs"]
#[allow(dead_code)]
mod native_visual;
#[path = "support/visual_renderer.rs"]
#[allow(dead_code)]
mod visual_renderer;

use live_visual::{
    ExternalPngCheckpointSpec, FocusCapture, LiveVisualRun, LiveVisualRunOptions,
    CHECKPOINT_DRAFT_VISIBLE, CHECKPOINT_RUN_FINISHED, CHECKPOINT_STARTUP,
};
use markers::{
    LIVE_OPERATOR_EMPTY_MARKER, LIVE_READY_NEXT_TURN_MARKER, OPERATOR_FILES_MARKER,
    REPLAY_DENSE_READY_MARKER, REPLAY_READY_MARKER, RUN_FINISHED_SHELL_MARKERS,
    STARTUP_COMMAND_PALETTE_MARKER, STARTUP_CONTINUE_HISTORY_MARKER,
    STARTUP_CONTINUE_HISTORY_READY_MARKER, STARTUP_HOME_ASCII_WORDMARK_MARKER,
    STARTUP_HOME_DENSE_VALUE_PROP_MARKER, STARTUP_HOME_SHORTCUT_MARKER,
    STARTUP_HOME_VALUE_PROP_MARKER, STARTUP_HOME_WORDMARK_MARKER, STARTUP_LAUNCHER_READY_MARKER,
    STARTUP_REPLAY_HISTORY_MARKER,
};
use native_visual::{
    default_native_visual_run_metadata, require_native_visual_availability,
    write_native_visual_summary, write_solid_png, GhosttyNativeHarness, NativeVisualAvailability,
    NativeVisualGrid,
};

const NATIVE_VISUAL_TEST_NAME: &str = "native_visual_ghostty_smoke";
const NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME: &str = "native_visual_startup_geometry_matrix";
const NATIVE_VISUAL_NAVIGATION_TEST_NAME: &str = "native_visual_navigation_views";
const NATIVE_VISUAL_SIDEBAR_TEST_NAME: &str = "native_visual_operator_sidebar_matrix";
const NATIVE_VISUAL_PERMISSION_TEST_NAME: &str = "native_visual_permission_lifecycle_parity";
const NATIVE_VISUAL_TRANSCRIPT_TEST_NAME: &str = "native_visual_transcript_parity";
const NATIVE_VISUAL_INNER_ENV: &str = "HARNESS_NATIVE_VISUAL_INNER";
const PLASMASHELL_PROCESS_NAME: &str = "plasmashell";
const NATIVE_VISUAL_SESSION_WIDTH: u16 = 2560;
const NATIVE_VISUAL_SESSION_HEIGHT: u16 = 1440;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const RUN_TIMEOUT: Duration = Duration::from_secs(20);
const NATIVE_VISUAL_PROMPT_TEXT: &str = "Hello from PTY";
const NATIVE_VISUAL_RESPONSE_MARKER: &str = "Hello world";
const PRIMARY_GRID: NativeVisualGrid = NativeVisualGrid {
    rows: 48,
    cols: 160,
};
const MINIMUM_GRID: NativeVisualGrid = NativeVisualGrid { rows: 24, cols: 80 };
const HALF_SCREEN_SPLIT_GRID: NativeVisualGrid = NativeVisualGrid { rows: 48, cols: 80 };
const SPLIT_TIER_GRID: NativeVisualGrid = NativeVisualGrid { rows: 40, cols: 96 };
const SIX_WINDOW_DENSE_GRID: NativeVisualGrid = NativeVisualGrid { rows: 18, cols: 60 };

#[test]
fn live_visual_external_png_checkpoint_supports_custom_namespace_and_prefix() {
    let artifact_root = unique_temp_dir("native-visual-artifacts");
    let source_png = artifact_root.join("source.png");
    write_solid_png(&source_png, 8, 4, [12, 34, 56]).expect("write source png");
    let mut visual_run = LiveVisualRun::new_in_with_options(
        artifact_root.join("custom-artifacts"),
        "live_visual_external_png_checkpoint_supports_custom_namespace_and_prefix",
        "run-001",
        LiveVisualRunOptions {
            run_metadata: json!({
                "backend": "ghostty",
                "capture_tool": "external_helper",
            }),
            artifact_namespace: "native-visual".to_string(),
            png_prefix: "native_visual".to_string(),
        },
    )
    .expect("create native visual run");

    let checkpoint_metadata = json!({
        "purpose": "external-png-unit-test",
    });
    let checkpoint = visual_run
        .capture_external_png_checkpoint(ExternalPngCheckpointSpec {
            checkpoint_id: CHECKPOINT_STARTUP,
            source_png_path: &source_png,
            screen_text: "Harness\nCtrl+p open\n",
            terminal_size: (2, 4),
            screen_markers: &["Harness", "Ctrl+p open"],
            focus: &FocusCapture::anchored_exact("Harness", 4, 1),
            metadata: Some(&checkpoint_metadata),
        })
        .expect("record external png checkpoint");

    assert!(
        checkpoint.png_path().exists(),
        "native visual PNG should be copied"
    );
    assert!(
        checkpoint
            .png_path()
            .to_string_lossy()
            .contains("native-visual/live_visual_external_png_checkpoint_supports_custom_namespace_and_prefix/run-001/native_visual_startup.png"),
        "PNG path should be namespaced under native-visual: {}",
        checkpoint.png_path().display()
    );

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(checkpoint.manifest_json_path()).expect("read manifest.json"),
    )
    .expect("parse manifest.json");
    assert_eq!(
        manifest.get("run_id").and_then(Value::as_str),
        Some("run-001")
    );
    assert_eq!(
        manifest
            .get("run_metadata")
            .and_then(|meta| meta.get("backend"))
            .and_then(Value::as_str),
        Some("ghostty")
    );
    let entry = manifest
        .get("checkpoints")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .expect("expected checkpoint entry");
    assert_eq!(
        entry.get("checkpoint_id").and_then(Value::as_str),
        Some(CHECKPOINT_STARTUP)
    );
    assert_eq!(
        entry
            .get("png_path")
            .and_then(Value::as_str)
            .map(|value| value.ends_with("native_visual_startup.png")),
        Some(true)
    );
    assert_eq!(
        entry
            .get("metadata")
            .and_then(|meta| meta.get("purpose"))
            .and_then(Value::as_str),
        Some("external-png-unit-test")
    );
}

#[test]
fn live_visual_external_png_checkpoint_rejects_unaligned_grid_dimensions() {
    let artifact_root = unique_temp_dir("native-visual-artifacts-unaligned");
    let source_png = artifact_root.join("unaligned-source.png");
    write_solid_png(&source_png, 7, 4, [90, 10, 10]).expect("write misaligned source png");
    let mut visual_run = LiveVisualRun::new_in_with_options(
        artifact_root.join("custom-artifacts"),
        "live_visual_external_png_checkpoint_rejects_unaligned_grid_dimensions",
        "run-002",
        LiveVisualRunOptions {
            run_metadata: json!({"backend": "ghostty"}),
            artifact_namespace: "native-visual".to_string(),
            png_prefix: "native_visual".to_string(),
        },
    )
    .expect("create native visual run");

    let err = visual_run
        .capture_external_png_checkpoint(ExternalPngCheckpointSpec {
            checkpoint_id: CHECKPOINT_STARTUP,
            source_png_path: &source_png,
            screen_text: "Harness\nCtrl+p open\n",
            terminal_size: (2, 4),
            screen_markers: &["Harness"],
            focus: &FocusCapture::anchored_exact("Harness", 4, 1),
            metadata: None,
        })
        .expect_err("unaligned native screenshot should fail closed");
    assert!(
        err.contains("does not map cleanly"),
        "expected unaligned grid error, got: {err}"
    );
}

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_ghostty_smoke() {
    run_native_visual_test(NATIVE_VISUAL_TEST_NAME, native_visual_ghostty_smoke_inner);
}

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_startup_geometry_matrix() {
    run_native_visual_test(
        NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME,
        native_visual_startup_geometry_matrix_inner,
    );
}

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_navigation_views() {
    run_native_visual_test(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        native_visual_navigation_views_inner,
    );
}

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_operator_sidebar_matrix() {
    run_native_visual_test(
        NATIVE_VISUAL_SIDEBAR_TEST_NAME,
        native_visual_operator_sidebar_matrix_inner,
    );
}

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_permission_lifecycle_parity() {
    run_native_visual_test(
        NATIVE_VISUAL_PERMISSION_TEST_NAME,
        native_visual_permission_lifecycle_parity_inner,
    );
}

#[test]
#[ignore = "requires HARNESS_NATIVE_VISUAL=1; runs inside a managed nested KWin/XWayland session"]
fn native_visual_transcript_parity() {
    run_native_visual_test(
        NATIVE_VISUAL_TRANSCRIPT_TEST_NAME,
        native_visual_transcript_parity_inner,
    );
}

fn run_native_visual_test(test_name: &str, inner: fn()) {
    if env::var("HARNESS_NATIVE_VISUAL").as_deref() != Ok("1") {
        return;
    }

    let _guard = native_visual_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if env::var(NATIVE_VISUAL_INNER_ENV).as_deref() != Ok("1") {
        run_native_visual_smoke_in_managed_session(test_name)
            .unwrap_or_else(|err| panic!("failed to run managed native visual session: {err}"));
        return;
    }

    match require_native_visual_availability()
        .unwrap_or_else(|err| panic!("native visual lane unavailable: {err}"))
    {
        NativeVisualAvailability::Disabled => return,
        NativeVisualAvailability::Ready => {}
    }

    inner();
}

fn native_visual_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn native_visual_ghostty_smoke_inner() {
    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();
    let session_dir = unique_temp_dir("native-visual-session");
    fs::create_dir_all(&session_dir).expect("create native visual session dir");
    let mut visual_run = new_native_visual_run(
        NATIVE_VISUAL_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "live_shell",
            "state": "happy_path",
            "scenario": "primary_smoke",
            "session_resolution": {
                "width": NATIVE_VISUAL_SESSION_WIDTH,
                "height": NATIVE_VISUAL_SESSION_HEIGHT,
            }
        }),
    );

    let harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn Ghostty native visual harness: {err}"));

    let startup_screen = harness
        .wait_for_text(STARTUP_HOME_SHORTCUT_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native visual startup marker did not appear: {err}"));
    assert!(startup_screen.contains(STARTUP_HOME_SHORTCUT_MARKER));
    let startup_checkpoint = harness
        .capture_checkpoint(
            &mut visual_run,
            CHECKPOINT_STARTUP,
            &[STARTUP_HOME_SHORTCUT_MARKER],
            &FocusCapture::anchored_exact(STARTUP_HOME_SHORTCUT_MARKER, 24, 3),
            Some(json!({"purpose": "startup-ready"})),
        )
        .unwrap_or_else(|err| panic!("failed to capture native startup screenshot: {err}"));
    assert!(startup_checkpoint.png_path().exists());

    harness
        .send_text(NATIVE_VISUAL_PROMPT_TEXT)
        .unwrap_or_else(|err| panic!("failed to send native draft text: {err}"));
    let draft_screen = harness
        .wait_for_text(NATIVE_VISUAL_PROMPT_TEXT, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native visual draft text did not appear: {err}"));
    assert!(draft_screen.contains(NATIVE_VISUAL_PROMPT_TEXT));
    let draft_checkpoint = harness
        .capture_checkpoint(
            &mut visual_run,
            CHECKPOINT_DRAFT_VISIBLE,
            &[STARTUP_HOME_SHORTCUT_MARKER, NATIVE_VISUAL_PROMPT_TEXT],
            &FocusCapture::anchored(NATIVE_VISUAL_PROMPT_TEXT, 24, 4),
            Some(json!({"purpose": "draft-visible"})),
        )
        .unwrap_or_else(|err| panic!("failed to capture native draft screenshot: {err}"));
    assert!(draft_checkpoint.png_path().exists());

    harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to submit native prompt: {err}"));
    let finished_screen = harness
        .wait_for_all_text(
            &["Success", NATIVE_VISUAL_RESPONSE_MARKER, "Ctrl+p commands"],
            RUN_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native visual run-finished markers did not appear: {err}"));
    assert!(finished_screen.contains("Success"));
    let run_finished_checkpoint = harness
        .capture_checkpoint(
            &mut visual_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "Success",
                NATIVE_VISUAL_RESPONSE_MARKER,
                "Ctrl+p commands",
                "Enter send",
            ],
            &FocusCapture::anchored_exact(NATIVE_VISUAL_RESPONSE_MARKER, 28, 6),
            Some(json!({"purpose": "run-finished"})),
        )
        .unwrap_or_else(|err| panic!("failed to capture native finished screenshot: {err}"));
    assert!(run_finished_checkpoint.png_path().exists());

    finalize_native_visual_run(harness, &visual_run);

    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(run_finished_checkpoint.manifest_json_path())
            .expect("read native visual manifest"),
    )
    .expect("parse native visual manifest");
    assert_eq!(
        manifest
            .get("checkpoints")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );
}

fn native_visual_startup_geometry_matrix_inner() {
    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();

    capture_startup_shell_scenario(StartupShellScenario {
        test_name: NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME,
        repo_root: &repo_root,
        harness_bin: &harness_bin,
        grid: PRIMARY_GRID.clone(),
        scenario: "startup_home_primary",
        ready_marker: STARTUP_HOME_SHORTCUT_MARKER,
        focus: FocusCapture::anchored_exact(STARTUP_HOME_WORDMARK_MARKER, 32, 8),
        markers: &[
            STARTUP_HOME_WORDMARK_MARKER,
            STARTUP_HOME_SHORTCUT_MARKER,
            STARTUP_HOME_VALUE_PROP_MARKER,
        ],
        metadata: json!({"family": "startup_shell", "state": "happy_path_primary"}),
    });
    capture_startup_shell_scenario(StartupShellScenario {
        test_name: NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME,
        repo_root: &repo_root,
        harness_bin: &harness_bin,
        grid: MINIMUM_GRID.clone(),
        scenario: "startup_quarter_tile",
        ready_marker: STARTUP_LAUNCHER_READY_MARKER,
        focus: FocusCapture::anchored_exact(STARTUP_LAUNCHER_READY_MARKER, 40, 2),
        markers: &[STARTUP_HOME_SHORTCUT_MARKER, STARTUP_HOME_VALUE_PROP_MARKER],
        metadata: json!({"family": "startup_shell", "state": "quarter_tile"}),
    });
    capture_startup_shell_scenario(StartupShellScenario {
        test_name: NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME,
        repo_root: &repo_root,
        harness_bin: &harness_bin,
        grid: HALF_SCREEN_SPLIT_GRID.clone(),
        scenario: "startup_split_tall",
        ready_marker: STARTUP_LAUNCHER_READY_MARKER,
        focus: FocusCapture::anchored_exact(STARTUP_LAUNCHER_READY_MARKER, 40, 2),
        markers: &[STARTUP_HOME_SHORTCUT_MARKER, STARTUP_HOME_VALUE_PROP_MARKER],
        metadata: json!({"family": "startup_shell", "state": "split_tall"}),
    });
    capture_startup_shell_scenario(StartupShellScenario {
        test_name: NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME,
        repo_root: &repo_root,
        harness_bin: &harness_bin,
        grid: SPLIT_TIER_GRID.clone(),
        scenario: "startup_split_tier",
        ready_marker: STARTUP_LAUNCHER_READY_MARKER,
        focus: FocusCapture::anchored_exact(STARTUP_LAUNCHER_READY_MARKER, 40, 2),
        markers: &[STARTUP_HOME_SHORTCUT_MARKER, STARTUP_HOME_VALUE_PROP_MARKER],
        metadata: json!({"family": "startup_shell", "state": "split_tier"}),
    });
    capture_startup_shell_scenario(StartupShellScenario {
        test_name: NATIVE_VISUAL_STARTUP_MATRIX_TEST_NAME,
        repo_root: &repo_root,
        harness_bin: &harness_bin,
        grid: SIX_WINDOW_DENSE_GRID.clone(),
        scenario: "startup_six_window",
        ready_marker: STARTUP_HOME_DENSE_VALUE_PROP_MARKER,
        focus: FocusCapture::anchored_exact(STARTUP_HOME_ASCII_WORDMARK_MARKER, 24, 8),
        markers: &[
            STARTUP_HOME_ASCII_WORDMARK_MARKER,
            STARTUP_HOME_DENSE_VALUE_PROP_MARKER,
        ],
        metadata: json!({"family": "startup_shell", "state": "six_window_dense"}),
    });
}

fn native_visual_navigation_views_inner() {
    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();

    let palette_session_dir = unique_temp_dir("native-visual-navigation-palette");
    fs::create_dir_all(&palette_session_dir).expect("create native visual palette session dir");
    let mut palette_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "startup_command_palette",
            "state": "happy_path",
            "scenario": "startup_command_palette",
        }),
    );
    let palette_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &palette_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native visual palette harness: {err}"));
    wait_for_startup_ready(&palette_harness, STARTUP_HOME_SHORTCUT_MARKER);
    open_startup_command_palette(&palette_harness);
    let palette_checkpoint = palette_harness
        .capture_checkpoint(
            &mut palette_run,
            CHECKPOINT_STARTUP,
            &[
                STARTUP_COMMAND_PALETTE_MARKER,
                "New session",
                STARTUP_CONTINUE_HISTORY_MARKER,
                STARTUP_REPLAY_HISTORY_MARKER,
            ],
            &FocusCapture::anchored_exact(STARTUP_COMMAND_PALETTE_MARKER, 56, 9),
            Some(json!({
                "purpose": "startup-command-palette",
                "family": "startup_command_palette",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| panic!("failed to capture native command palette screenshot: {err}"));
    assert!(palette_checkpoint.png_path().exists());
    finalize_native_visual_run(palette_harness, &palette_run);

    let history_session_dir = unique_temp_dir("native-visual-navigation-history");
    fs::create_dir_all(&history_session_dir).expect("create native visual history session dir");
    write_quiescent_resume_fixture(&history_session_dir, "run_resume_quiescent");

    let mut replay_history_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "startup_session_history",
            "state": "replay_history",
            "scenario": "startup_replay_history",
        }),
    );
    let replay_history_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &history_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native visual replay history harness: {err}"));
    wait_for_startup_ready(&replay_history_harness, STARTUP_HOME_SHORTCUT_MARKER);
    open_startup_session_history(
        &replay_history_harness,
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    let replay_history_screen = replay_history_harness
        .wait_for_all_text(
            &[
                STARTUP_REPLAY_HISTORY_MARKER,
                "Read-only replays",
                "interactive and prompt runs stay available",
            ],
            STARTUP_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native replay history markers did not stabilize: {err}"));
    assert!(replay_history_screen.contains("interactive and prompt runs stay available"));
    let replay_history_checkpoint = replay_history_harness
        .capture_checkpoint(
            &mut replay_history_run,
            CHECKPOINT_STARTUP,
            &[
                STARTUP_REPLAY_HISTORY_MARKER,
                "Read-only replays",
                "interactive and prompt runs stay available",
            ],
            &FocusCapture::anchored_exact(STARTUP_REPLAY_HISTORY_MARKER, 28, 2),
            Some(json!({
                "purpose": "startup-replay-history",
                "family": "startup_session_history",
                "state": "replay_history",
            })),
        )
        .unwrap_or_else(|err| panic!("failed to capture native replay history screenshot: {err}"));
    assert!(replay_history_checkpoint.png_path().exists());
    replay_history_harness
        .send_key("Escape")
        .unwrap_or_else(|err| panic!("failed to close replay history overlay: {err}"));
    wait_for_startup_ready(&replay_history_harness, STARTUP_LAUNCHER_READY_MARKER);
    finalize_native_visual_run(replay_history_harness, &replay_history_run);

    let mut continue_history_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "startup_session_history",
            "state": "continue_history",
            "scenario": "startup_continue_history",
        }),
    );
    let continue_history_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &history_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native visual continue history harness: {err}"));
    wait_for_startup_ready(&continue_history_harness, STARTUP_HOME_SHORTCUT_MARKER);
    open_startup_session_history(
        &continue_history_harness,
        "resume",
        STARTUP_CONTINUE_HISTORY_READY_MARKER,
    );
    let continue_history_checkpoint = continue_history_harness
        .capture_checkpoint(
            &mut continue_history_run,
            CHECKPOINT_STARTUP,
            &[
                STARTUP_CONTINUE_HISTORY_MARKER,
                STARTUP_CONTINUE_HISTORY_READY_MARKER,
                "Interactive histories",
            ],
            &FocusCapture::anchored_exact(STARTUP_CONTINUE_HISTORY_READY_MARKER, 28, 3),
            Some(json!({
                "purpose": "startup-continue-history",
                "family": "startup_session_history",
                "state": "continue_history",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native continue history screenshot: {err}")
        });
    assert!(continue_history_checkpoint.png_path().exists());
    continue_history_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to continue selected quiescent session: {err}"));
    let continued_screen = continue_history_harness
        .wait_for_text("Continued runtime:", STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native continued runtime marker did not appear: {err}"));
    assert!(continued_screen.contains("Continued runtime:"));
    let mut continue_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "continue_session",
            "state": "happy_path",
            "scenario": "continue_quiescent_session",
        }),
    );
    let continue_checkpoint = continue_history_harness
        .capture_checkpoint(
            &mut continue_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "Enter send",
                LIVE_OPERATOR_EMPTY_MARKER,
                "Continued runtime:",
            ],
            &FocusCapture::anchored_exact("Continued runtime:", 18, 1),
            Some(json!({
                "purpose": "continue-quiescent-session",
                "family": "continue_session",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native continued session screenshot: {err}")
        });
    assert!(continue_checkpoint.png_path().exists());
    let continue_summary = continue_history_harness
        .cleanup()
        .unwrap_or_else(|err| panic!("failed to clean up continued-session native harness: {err}"));
    write_native_visual_summary(continue_history_run.run_dir(), &continue_summary)
        .unwrap_or_else(|err| panic!("failed to write continue-history native summary: {err}"));
    write_native_visual_summary(continue_run.run_dir(), &continue_summary)
        .unwrap_or_else(|err| panic!("failed to write continued-session native summary: {err}"));

    let replay_session_dir = unique_temp_dir("native-visual-navigation-replay");
    fs::create_dir_all(&replay_session_dir).expect("create native visual replay session dir");
    seed_completed_mock_session(
        &repo_root,
        &harness_bin,
        &replay_session_dir,
        "shell parity task",
    );
    let mut replay_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "replay_shell",
            "state": "happy_path",
            "scenario": "session_shell_primary_replay",
        }),
    );
    let replay_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &replay_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native visual replay harness: {err}"));
    wait_for_startup_ready(&replay_harness, STARTUP_HOME_SHORTCUT_MARKER);
    open_startup_session_history(&replay_harness, "replay", STARTUP_REPLAY_HISTORY_MARKER);
    replay_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to select replay session from history: {err}"));
    let replay_screen = replay_harness
        .wait_for_text(REPLAY_DENSE_READY_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native replay shell marker did not appear: {err}"));
    assert!(replay_screen.contains(REPLAY_DENSE_READY_MARKER));
    let replay_checkpoint = replay_harness
        .capture_checkpoint(
            &mut replay_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                REPLAY_DENSE_READY_MARKER,
                "shell parity task",
                LIVE_OPERATOR_EMPTY_MARKER,
                "Replay is read-only.",
            ],
            &FocusCapture::anchored_exact(REPLAY_DENSE_READY_MARKER, 20, 2),
            Some(json!({
                "purpose": "session-shell-primary-replay",
                "family": "replay_shell",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| panic!("failed to capture native replay shell screenshot: {err}"));
    assert!(replay_checkpoint.png_path().exists());
    finalize_native_visual_run(replay_harness, &replay_run);

    let blocked_session_dir = unique_temp_dir("native-visual-navigation-blocked");
    fs::create_dir_all(&blocked_session_dir).expect("create native visual blocked session dir");
    write_active_blocked_fixture(&blocked_session_dir, "run_active_blocked");
    write_corrupt_blocked_fixture(&blocked_session_dir, "run_unrestorable_blocked");

    let mut active_reject_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "continue_session",
            "state": "failure_path",
            "scenario": "continue_rejected_active",
        }),
    );
    let active_reject_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &blocked_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native active-rejection harness: {err}"));
    wait_for_startup_ready(&active_reject_harness, STARTUP_LAUNCHER_READY_MARKER);
    open_startup_session_history(
        &active_reject_harness,
        "resume",
        "tasks are still in flight",
    );
    active_reject_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to attempt continue on active session: {err}"));
    let active_reject_screen = active_reject_harness
        .wait_for_text("tasks are still in flight", STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native active-session rejection did not appear: {err}"));
    assert!(active_reject_screen.contains("tasks are still in flight"));
    let active_reject_checkpoint = active_reject_harness
        .capture_checkpoint(
            &mut active_reject_run,
            CHECKPOINT_RUN_FINISHED,
            &[STARTUP_CONTINUE_HISTORY_MARKER, "tasks are still in flight"],
            &FocusCapture::anchored_exact("tasks are still in flight", 28, 1),
            Some(json!({
                "purpose": "continue-rejected-active",
                "family": "continue_session",
                "state": "failure_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native active-session rejection screenshot: {err}")
        });
    assert!(active_reject_checkpoint.png_path().exists());
    active_reject_harness
        .send_key("Escape")
        .unwrap_or_else(|err| panic!("failed to close native active rejection overlay: {err}"));
    wait_for_startup_ready(&active_reject_harness, STARTUP_LAUNCHER_READY_MARKER);
    finalize_native_visual_run(active_reject_harness, &active_reject_run);

    let mut unrestorable_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "continue_session",
            "state": "failure_path",
            "scenario": "continue_rejected_unrestorable",
        }),
    );
    let unrestorable_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &blocked_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native unrestorable harness: {err}"));
    wait_for_startup_ready(&unrestorable_harness, STARTUP_LAUNCHER_READY_MARKER);
    open_startup_session_history(&unrestorable_harness, "resume", "events unavailable");
    move_list_selection(&unrestorable_harness, 1);
    unrestorable_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to attempt continue on unrestorable session: {err}"));
    let unrestorable_screen = unrestorable_harness
        .wait_for_text("events unavailable", STARTUP_TIMEOUT)
        .unwrap_or_else(|err| {
            panic!("native unrestorable-session rejection did not appear: {err}")
        });
    assert!(unrestorable_screen.contains("events unavailable"));
    let unrestorable_checkpoint = unrestorable_harness
        .capture_checkpoint(
            &mut unrestorable_run,
            CHECKPOINT_RUN_FINISHED,
            &[STARTUP_CONTINUE_HISTORY_MARKER, "events unavailable"],
            &FocusCapture::anchored_exact("events unavailable", 28, 1),
            Some(json!({
                "purpose": "continue-rejected-unrestorable",
                "family": "continue_session",
                "state": "failure_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native unrestorable-session rejection screenshot: {err}")
        });
    assert!(unrestorable_checkpoint.png_path().exists());
    unrestorable_harness
        .send_key("Escape")
        .unwrap_or_else(|err| {
            panic!("failed to close native unrestorable rejection overlay: {err}")
        });
    wait_for_startup_ready(&unrestorable_harness, STARTUP_LAUNCHER_READY_MARKER);
    finalize_native_visual_run(unrestorable_harness, &unrestorable_run);

    let child_session_dir = unique_temp_dir("native-visual-navigation-child");
    fs::create_dir_all(&child_session_dir)
        .expect("create native visual child navigation session dir");
    let parent_session_id = "000_parent_navigation";
    let child_session_id = "zzz_child_navigation";
    write_child_navigation_fixture(&child_session_dir, parent_session_id, child_session_id);
    let mut child_navigation_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "replay_shell",
            "state": "child_navigation",
            "scenario": "child_session_navigation",
        }),
    );
    let child_navigation_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &child_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native child-navigation harness: {err}"));
    wait_for_startup_ready(&child_navigation_harness, STARTUP_LAUNCHER_READY_MARKER);
    open_startup_session_history(
        &child_navigation_harness,
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    child_navigation_harness
        .send_text(parent_session_id)
        .unwrap_or_else(|err| {
            panic!("failed to filter native child-navigation replay history: {err}")
        });
    child_navigation_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to select native parent replay session: {err}"));
    let parent_replay_screen = child_navigation_harness
        .wait_for_text(
            "Parent session can jump to child transcripts.",
            STARTUP_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native parent replay screen did not appear: {err}"));
    assert!(parent_replay_screen.contains("Parent session can jump to child transcripts."));
    child_navigation_harness
        .send_key("C-]")
        .unwrap_or_else(|err| panic!("failed to navigate to native child session: {err}"));
    let child_screen = child_navigation_harness
        .wait_for_text("Child session captured sidebar parity.", STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native child replay screen did not appear: {err}"));
    assert!(child_screen.contains(child_session_id));
    let child_navigation_checkpoint = child_navigation_harness
        .capture_checkpoint(
            &mut child_navigation_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                REPLAY_DENSE_READY_MARKER,
                child_session_id,
                "Child session captured sidebar parity.",
            ],
            &FocusCapture::anchored_exact("Child session captured sidebar parity.", 30, 6),
            Some(json!({
                "purpose": "child-session-navigation",
                "family": "replay_shell",
                "state": "child_navigation",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native child navigation screenshot: {err}")
        });
    assert!(child_navigation_checkpoint.png_path().exists());
    child_navigation_harness
        .send_key("Escape")
        .unwrap_or_else(|err| {
            panic!("failed to navigate back to native parent replay session: {err}")
        });
    let parent_screen = child_navigation_harness
        .wait_for_text(
            "Parent session can jump to child transcripts.",
            STARTUP_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native parent replay screen did not reappear: {err}"));
    assert!(parent_screen.contains("Parent session asks the child to inspect parity"));
    finalize_native_visual_run(child_navigation_harness, &child_navigation_run);

    let replay_read_only_dir = unique_temp_dir("native-visual-navigation-replay-read-only");
    fs::create_dir_all(&replay_read_only_dir).expect("create native visual replay-read-only dir");
    let replay_run_dir = write_replay_fixture(&replay_read_only_dir, "run_replay_safe");
    let replay_events_path = replay_run_dir.join("events.jsonl");
    let replay_before =
        fs::read_to_string(&replay_events_path).expect("read native replay fixture before launch");
    let mut replay_read_only_run = new_native_visual_run(
        NATIVE_VISUAL_NAVIGATION_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "replay",
            "state": "failure_path",
            "scenario": "replay_read_only",
        }),
    );
    let replay_read_only_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &replay_read_only_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native replay-read-only harness: {err}"));
    wait_for_startup_ready(&replay_read_only_harness, STARTUP_LAUNCHER_READY_MARKER);
    open_startup_session_history(
        &replay_read_only_harness,
        "replay",
        STARTUP_REPLAY_HISTORY_MARKER,
    );
    replay_read_only_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to select native replay-read-only session: {err}"));
    let replay_read_only_screen = replay_read_only_harness
        .wait_for_text(REPLAY_READY_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native replay-read-only shell did not appear: {err}"));
    assert!(replay_read_only_screen.contains(REPLAY_READY_MARKER));
    replay_read_only_harness
        .send_key("Tab")
        .unwrap_or_else(|err| panic!("failed to focus replay details pane: {err}"));
    replay_read_only_harness
        .send_key("Tab")
        .unwrap_or_else(|err| panic!("failed to attempt replay prompt focus: {err}"));
    replay_read_only_harness
        .send_text("blocked in replay")
        .unwrap_or_else(|err| panic!("failed to type blocked replay text: {err}"));
    replay_read_only_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to attempt blocked replay submit: {err}"));
    let replay_stable_screen = replay_read_only_harness
        .wait_for_text(REPLAY_READY_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native replay shell did not stay read-only: {err}"));
    assert!(
        replay_stable_screen.contains("Replay · read-only · run run_replay_safe"),
        "expected replay header in screen:\n{replay_stable_screen}"
    );
    assert!(
        replay_stable_screen.contains(REPLAY_READY_MARKER),
        "expected replay quit affordance in screen:\n{replay_stable_screen}"
    );
    assert!(
        replay_stable_screen.contains("Replay is read-only"),
        "expected replay read-only marker in screen:\n{replay_stable_screen}"
    );
    assert!(
        !replay_stable_screen.contains("Enter send"),
        "replay screen must stay read-only with no composer submit affordance:\n{replay_stable_screen}"
    );
    let replay_read_only_checkpoint = replay_read_only_harness
        .capture_checkpoint(
            &mut replay_read_only_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                REPLAY_READY_MARKER,
                "run_replay_safe",
                REPLAY_DENSE_READY_MARKER,
                "Replay is read-only",
            ],
            &FocusCapture::anchored_exact(REPLAY_READY_MARKER, 20, 2),
            Some(json!({
                "purpose": "replay-read-only",
                "family": "replay",
                "state": "failure_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native replay read-only screenshot: {err}")
        });
    assert!(replay_read_only_checkpoint.png_path().exists());
    finalize_native_visual_run(replay_read_only_harness, &replay_read_only_run);
    let replay_after =
        fs::read_to_string(&replay_events_path).expect("read native replay fixture after launch");
    assert_eq!(
        replay_after, replay_before,
        "native replay mode must not append to events.jsonl"
    );
}

fn native_visual_operator_sidebar_matrix_inner() {
    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();

    capture_operator_sidebar_scenario(
        NATIVE_VISUAL_SIDEBAR_TEST_NAME,
        &repo_root,
        &harness_bin,
        PRIMARY_GRID.clone(),
        "operator_sidebar_primary",
        json!({"family": "operator_sidebar", "state": "happy_path"}),
    );
    capture_operator_sidebar_scenario(
        NATIVE_VISUAL_SIDEBAR_TEST_NAME,
        &repo_root,
        &harness_bin,
        HALF_SCREEN_SPLIT_GRID.clone(),
        "operator_sidebar_split_tall",
        json!({"family": "operator_sidebar", "state": "split_tall"}),
    );
    capture_operator_sidebar_scenario(
        NATIVE_VISUAL_SIDEBAR_TEST_NAME,
        &repo_root,
        &harness_bin,
        SPLIT_TIER_GRID.clone(),
        "operator_sidebar_split_tier",
        json!({"family": "operator_sidebar", "state": "split_tier"}),
    );
    capture_operator_sidebar_parity_scenario(&repo_root, &harness_bin);
}

fn native_visual_permission_lifecycle_parity_inner() {
    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();
    let session_dir = unique_temp_dir("native-visual-permission");
    fs::create_dir_all(&session_dir).expect("create native visual permission session dir");

    let mut permission_run = new_native_visual_run(
        NATIVE_VISUAL_PERMISSION_TEST_NAME,
        &MINIMUM_GRID,
        json!({
            "family": "permission",
            "state": "happy_path",
            "scenario": "permission_overlay_parity",
            "session_resolution": {
                "width": NATIVE_VISUAL_SESSION_WIDTH,
                "height": NATIVE_VISUAL_SESSION_HEIGHT,
            }
        }),
    );
    let harness = GhosttyNativeHarness::spawn_with_args(
        &harness_bin,
        &repo_root,
        &session_dir,
        MINIMUM_GRID.clone(),
        &["--scenario", "golden_path_interactive"],
    )
    .unwrap_or_else(|err| panic!("failed to spawn native permission scenario harness: {err}"));
    harness
        .send_text("keep this draft")
        .unwrap_or_else(|err| panic!("failed to write preserved native permission draft: {err}"));
    let permission_screen = harness
        .wait_for_all_text(
            &["Permission Requested", "keep this draft"],
            STARTUP_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native permission overlay did not preserve draft: {err}"));
    assert!(permission_screen.contains("keep this draft"));
    harness.send_key("C-p").unwrap_or_else(|err| {
        panic!("failed to attempt palette under native permission overlay: {err}")
    });
    harness.send_text("/").unwrap_or_else(|err| {
        panic!("failed to attempt slash under native permission overlay: {err}")
    });
    let stable_permission_screen = harness
        .wait_for_all_text(
            &["Permission Requested", "keep this draft"],
            STARTUP_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native permission overlay did not remain stable: {err}"));
    assert!(stable_permission_screen.contains("FAIL CLOSED"));
    assert!(stable_permission_screen.contains("SESSION PAUSED"));
    let permission_checkpoint = harness
        .capture_checkpoint(
            &mut permission_run,
            CHECKPOINT_STARTUP,
            &[
                "Permission Requested",
                "FAIL CLOSED",
                "SESSION PAUSED",
                "keep this draft",
            ],
            &FocusCapture::anchored_exact("Permission Requested", 24, 1),
            Some(json!({
                "purpose": "permission-overlay-parity",
                "family": "permission",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native permission overlay screenshot: {err}")
        });
    assert!(permission_checkpoint.png_path().exists());

    harness
        .send_key("C-y")
        .unwrap_or_else(|err| panic!("failed to approve native permission prompt: {err}"));
    let inline_completion_screen = harness
        .wait_for_text(LIVE_READY_NEXT_TURN_MARKER, RUN_TIMEOUT)
        .unwrap_or_else(|err| panic!("native inline-completion shell did not appear: {err}"));
    assert!(inline_completion_screen.contains(LIVE_READY_NEXT_TURN_MARKER));
    let mut inline_completion_run = new_native_visual_run(
        NATIVE_VISUAL_PERMISSION_TEST_NAME,
        &MINIMUM_GRID,
        json!({
            "family": "live_shell",
            "state": "inline_completion",
            "scenario": "inline_completion_shell",
            "session_resolution": {
                "width": NATIVE_VISUAL_SESSION_WIDTH,
                "height": NATIVE_VISUAL_SESSION_HEIGHT,
            }
        }),
    );
    let inline_completion_checkpoint = harness
        .capture_checkpoint(
            &mut inline_completion_run,
            CHECKPOINT_RUN_FINISHED,
            RUN_FINISHED_SHELL_MARKERS,
            &FocusCapture::anchored_exact(LIVE_READY_NEXT_TURN_MARKER, 20, 1),
            Some(json!({
                "purpose": "inline-completion-shell",
                "family": "live_shell",
                "state": "inline_completion",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native inline-completion screenshot: {err}")
        });
    assert!(inline_completion_checkpoint.png_path().exists());

    let summary = harness.cleanup().unwrap_or_else(|err| {
        panic!("failed to clean up native permission scenario harness: {err}")
    });
    write_native_visual_summary(permission_run.run_dir(), &summary)
        .unwrap_or_else(|err| panic!("failed to write native permission summary: {err}"));
    write_native_visual_summary(inline_completion_run.run_dir(), &summary)
        .unwrap_or_else(|err| panic!("failed to write native inline-completion summary: {err}"));
}

fn native_visual_transcript_parity_inner() {
    let repo_root = repo_root();
    let harness_bin = resolve_harness_bin();

    let primary_session_dir = unique_temp_dir("native-visual-transcript-primary");
    fs::create_dir_all(&primary_session_dir).expect("create native visual transcript primary dir");
    write_native_tool_parity_fixture(&primary_session_dir, "run_native_tool_parity");
    let mut transcript_run = new_native_visual_run(
        NATIVE_VISUAL_TRANSCRIPT_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "transcript_shell",
            "state": "happy_path",
            "scenario": "native_tool_parity_primary",
        }),
    );
    let transcript_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &primary_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native transcript parity harness: {err}"));
    wait_for_startup_ready(&transcript_harness, STARTUP_HOME_SHORTCUT_MARKER);
    open_startup_session_history(&transcript_harness, "replay", STARTUP_REPLAY_HISTORY_MARKER);
    transcript_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to select native transcript replay session: {err}"));
    transcript_harness
        .wait_for_text(REPLAY_DENSE_READY_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native transcript replay shell did not appear: {err}"));
    let transcript_screen =
        run_session_palette_command(&transcript_harness, "show timestamps", "14:37");
    assert!(transcript_screen.contains("Thinking: Drafting the inline parity pass."));
    let thinking_checkpoint = transcript_harness
        .capture_checkpoint(
            &mut transcript_run,
            CHECKPOINT_STARTUP,
            &[
                "Thinking: Drafting the inline parity pass.",
                "Inline transcript parity is easier to scan now.",
                "Bring native tool parity inline",
                REPLAY_DENSE_READY_MARKER,
            ],
            &FocusCapture::anchored_exact("Thinking: Drafting the inline parity pass.", 40, 5),
            Some(json!({
                "purpose": "native-tool-parity-thinking",
                "family": "transcript_shell",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native transcript thinking screenshot: {err}")
        });
    assert!(thinking_checkpoint.png_path().exists());
    let task_checkpoint = transcript_harness
        .capture_checkpoint(
            &mut transcript_run,
            CHECKPOINT_DRAFT_VISIBLE,
            &[
                "Bring native tool parity inline",
                "Read src/ui.rs [offset=12, limit=24] · 14:35 · 1.2s",
                "Spawn researcher · audit transcript parity",
                "14:36 · 1.6s",
                "foreground · agent_worker · req_child · completed · 3 child tool calls",
            ],
            &FocusCapture::anchored_exact("Spawn researcher", 34, 6),
            Some(json!({
                "purpose": "native-tool-parity-task-row",
                "family": "transcript_shell",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native transcript task-row screenshot: {err}")
        });
    assert!(task_checkpoint.png_path().exists());
    let fetch_checkpoint = transcript_harness
        .capture_checkpoint(
            &mut transcript_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "Fetch https://example.test/report.pdf",
                "14:37 · 2.4s",
                "Attachment · artifacts/toolcalls/tc-fetch/web.fetch.pdf · digest-fetch-artifact",
                "exit code: 1",
                "stderr: snapshot mismatch",
            ],
            &FocusCapture::anchored_exact("Fetch https://example.test/report.pdf", 42, 7),
            Some(json!({
                "purpose": "native-tool-parity-fetch-row",
                "family": "transcript_shell",
                "state": "happy_path",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native transcript fetch-row screenshot: {err}")
        });
    assert!(fetch_checkpoint.png_path().exists());
    finalize_native_visual_run(transcript_harness, &transcript_run);

    let mcp_session_dir = unique_temp_dir("native-visual-transcript-mcp");
    fs::create_dir_all(&mcp_session_dir).expect("create native visual transcript mcp dir");
    write_native_tool_parity_fixture(&mcp_session_dir, "run_native_tool_parity_mcp");
    let mut mcp_run = new_native_visual_run(
        NATIVE_VISUAL_TRANSCRIPT_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "transcript_shell",
            "state": "mcp_background",
            "scenario": "native_tool_parity_mcp_background",
        }),
    );
    let mcp_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &mcp_session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native transcript mcp harness: {err}"));
    wait_for_startup_ready(&mcp_harness, STARTUP_HOME_SHORTCUT_MARKER);
    open_startup_session_history(&mcp_harness, "replay", STARTUP_REPLAY_HISTORY_MARKER);
    mcp_harness.send_enter().unwrap_or_else(|err| {
        panic!("failed to select native transcript mcp replay session: {err}")
    });
    let mcp_screen = run_session_palette_command(&mcp_harness, "show timestamps", "14:37");
    assert!(mcp_screen.contains("MCP docs-rs · search_in_crate"));
    let mcp_checkpoint = mcp_harness
        .capture_checkpoint(
            &mut mcp_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "MCP docs-rs · search_in_crate",
                "14:37 · 650ms",
                REPLAY_DENSE_READY_MARKER,
            ],
            &FocusCapture::anchored_exact("MCP docs-rs", 44, 6),
            Some(json!({
                "purpose": "native-tool-parity-mcp-background",
                "family": "transcript_shell",
                "state": "mcp_background",
            })),
        )
        .unwrap_or_else(|err| panic!("failed to capture native transcript mcp screenshot: {err}"));
    assert!(mcp_checkpoint.png_path().exists());
    finalize_native_visual_run(mcp_harness, &mcp_run);

    let dense_session_dir = unique_temp_dir("native-visual-transcript-dense");
    fs::create_dir_all(&dense_session_dir).expect("create native visual transcript dense dir");
    write_native_tool_parity_fixture(&dense_session_dir, "run_native_tool_parity_dense");
    let mut dense_run = new_native_visual_run(
        NATIVE_VISUAL_TRANSCRIPT_TEST_NAME,
        &SIX_WINDOW_DENSE_GRID,
        json!({
            "family": "transcript_shell",
            "state": "dense_parity",
            "scenario": "native_tool_parity_dense",
        }),
    );
    let dense_harness = GhosttyNativeHarness::spawn_mock(
        &harness_bin,
        &repo_root,
        &dense_session_dir,
        SIX_WINDOW_DENSE_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native transcript dense harness: {err}"));
    wait_for_startup_ready(&dense_harness, STARTUP_HOME_DENSE_VALUE_PROP_MARKER);
    open_startup_session_history(&dense_harness, "replay", STARTUP_REPLAY_HISTORY_MARKER);
    dense_harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to select native dense replay session: {err}"));
    let dense_screen = run_session_palette_command(&dense_harness, "show timestamps", "14:37");
    assert!(!dense_screen.contains("Event log"));
    let dense_checkpoint = dense_harness
        .capture_checkpoint(
            &mut dense_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "Compat alias · webfetch → web.fetch",
                "artifacts/toolcalls/tc-fetch/web.fetch.pdf",
                "cargo test -p harness-tui",
                "14:37",
                REPLAY_DENSE_READY_MARKER,
            ],
            &FocusCapture::anchored_exact("Compat alias · webfetch → web.fetch", 30, 6),
            Some(json!({
                "purpose": "native-tool-parity-dense",
                "family": "transcript_shell",
                "state": "dense_parity",
            })),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native transcript dense screenshot: {err}")
        });
    assert!(dense_checkpoint.png_path().exists());
    finalize_native_visual_run(dense_harness, &dense_run);
}

struct StartupShellScenario<'a> {
    test_name: &'a str,
    repo_root: &'a Path,
    harness_bin: &'a Path,
    grid: NativeVisualGrid,
    scenario: &'a str,
    ready_marker: &'a str,
    focus: FocusCapture,
    markers: &'a [&'a str],
    metadata: Value,
}

fn capture_startup_shell_scenario(scenario: StartupShellScenario<'_>) {
    let session_dir = unique_temp_dir("native-visual-startup-matrix");
    fs::create_dir_all(&session_dir).expect("create native visual startup matrix session dir");
    let mut visual_run = new_native_visual_run(
        scenario.test_name,
        &scenario.grid,
        merge_metadata(
            scenario.metadata,
            json!({
                "scenario": scenario.scenario,
                "session_resolution": {
                    "width": NATIVE_VISUAL_SESSION_WIDTH,
                    "height": NATIVE_VISUAL_SESSION_HEIGHT,
                }
            }),
        ),
    );
    let harness = GhosttyNativeHarness::spawn_mock(
        scenario.harness_bin,
        scenario.repo_root,
        &session_dir,
        scenario.grid.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native visual startup matrix harness: {err}"));
    let startup_screen = harness
        .wait_for_text(scenario.ready_marker, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| {
            panic!(
                "native visual startup marker {:?} did not appear: {err}",
                scenario.ready_marker
            )
        });
    assert!(startup_screen.contains(scenario.ready_marker));
    let checkpoint = harness
        .capture_checkpoint(
            &mut visual_run,
            CHECKPOINT_STARTUP,
            scenario.markers,
            &scenario.focus,
            Some(json!({"purpose": scenario.scenario})),
        )
        .unwrap_or_else(|err| panic!("failed to capture native startup matrix screenshot: {err}"));
    assert!(checkpoint.png_path().exists());
    finalize_native_visual_run(harness, &visual_run);
}

fn capture_operator_sidebar_scenario(
    test_name: &str,
    repo_root: &Path,
    harness_bin: &Path,
    grid: NativeVisualGrid,
    scenario: &str,
    metadata: Value,
) {
    let session_dir = unique_temp_dir("native-visual-sidebar");
    fs::create_dir_all(&session_dir).expect("create native visual sidebar session dir");
    let mut visual_run = new_native_visual_run(
        test_name,
        &grid,
        merge_metadata(
            metadata,
            json!({
                "scenario": scenario,
                "session_resolution": {
                    "width": NATIVE_VISUAL_SESSION_WIDTH,
                    "height": NATIVE_VISUAL_SESSION_HEIGHT,
                }
            }),
        ),
    );
    let harness = GhosttyNativeHarness::spawn_mock(harness_bin, repo_root, &session_dir, grid)
        .unwrap_or_else(|err| panic!("failed to spawn native visual sidebar harness: {err}"));
    wait_for_startup_ready(&harness, STARTUP_HOME_SHORTCUT_MARKER);
    harness
        .send_text("shell parity task")
        .unwrap_or_else(|err| panic!("failed to submit native sidebar prompt text: {err}"));
    harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to submit native sidebar prompt: {err}"));
    let finished_screen = harness
        .wait_for_all_text(&["Success", "Ctrl+p commands", "Enter send"], RUN_TIMEOUT)
        .unwrap_or_else(|err| panic!("native sidebar run-finished markers did not appear: {err}"));
    assert!(finished_screen.contains("Success"));
    show_operator_sidebar(&harness, LIVE_OPERATOR_EMPTY_MARKER);
    let checkpoint = harness
        .capture_checkpoint(
            &mut visual_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "shell parity task",
                LIVE_OPERATOR_EMPTY_MARKER,
                "Current runtime:",
                LIVE_READY_NEXT_TURN_MARKER,
                "Enter send",
            ],
            &FocusCapture::anchored_exact(LIVE_OPERATOR_EMPTY_MARKER, 20, 6),
            Some(json!({"purpose": scenario})),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native operator sidebar screenshot: {err}")
        });
    assert!(checkpoint.png_path().exists());
    finalize_native_visual_run(harness, &visual_run);
}

fn capture_operator_sidebar_parity_scenario(repo_root: &Path, harness_bin: &Path) {
    let session_dir = unique_temp_dir("native-visual-sidebar-parity");
    fs::create_dir_all(&session_dir).expect("create native visual sidebar parity session dir");
    let mut visual_run = new_native_visual_run(
        NATIVE_VISUAL_SIDEBAR_TEST_NAME,
        &PRIMARY_GRID,
        json!({
            "family": "operator_sidebar",
            "state": "parity",
            "scenario": "opencode_sidebar_session_parity",
            "session_resolution": {
                "width": NATIVE_VISUAL_SESSION_WIDTH,
                "height": NATIVE_VISUAL_SESSION_HEIGHT,
            }
        }),
    );
    let harness = GhosttyNativeHarness::spawn_mock(
        harness_bin,
        repo_root,
        &session_dir,
        PRIMARY_GRID.clone(),
    )
    .unwrap_or_else(|err| panic!("failed to spawn native visual sidebar parity harness: {err}"));
    wait_for_startup_ready(&harness, STARTUP_HOME_SHORTCUT_MARKER);
    harness
        .send_text("shell parity task")
        .unwrap_or_else(|err| panic!("failed to submit native sidebar parity prompt: {err}"));
    harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to run native sidebar parity prompt: {err}"));
    let finished_screen = harness
        .wait_for_all_text(
            &[
                "shell parity task",
                "Shell parity looks good.",
                LIVE_READY_NEXT_TURN_MARKER,
            ],
            RUN_TIMEOUT,
        )
        .unwrap_or_else(|err| panic!("native sidebar parity shell did not finish: {err}"));
    assert!(finished_screen.contains("Shell parity looks good."));
    show_operator_sidebar(&harness, "Current runtime:");
    let checkpoint = harness
        .capture_checkpoint(
            &mut visual_run,
            CHECKPOINT_RUN_FINISHED,
            &[
                "shell parity task",
                "Shell parity looks good.",
                LIVE_OPERATOR_EMPTY_MARKER,
                "Current runtime:",
                OPERATOR_FILES_MARKER,
                LIVE_READY_NEXT_TURN_MARKER,
                "Enter send",
            ],
            &FocusCapture::anchored_exact("Current runtime:", 18, 8),
            Some(json!({"purpose": "opencode-sidebar-session-parity"})),
        )
        .unwrap_or_else(|err| {
            panic!("failed to capture native operator sidebar parity screenshot: {err}")
        });
    assert!(checkpoint.png_path().exists());
    finalize_native_visual_run(harness, &visual_run);
}

fn wait_for_startup_ready(harness: &GhosttyNativeHarness, marker: &str) {
    let startup_screen = harness
        .wait_for_text(marker, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native startup marker {marker:?} did not appear: {err}"));
    assert!(startup_screen.contains(marker));
}

fn open_startup_command_palette(harness: &GhosttyNativeHarness) {
    harness
        .send_key("C-p")
        .unwrap_or_else(|err| panic!("failed to open native startup command palette: {err}"));
    let palette_screen = harness
        .wait_for_text(STARTUP_COMMAND_PALETTE_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native startup command palette did not appear: {err}"));
    assert!(palette_screen.contains(STARTUP_COMMAND_PALETTE_MARKER));
}

fn open_startup_session_history(harness: &GhosttyNativeHarness, command: &str, marker: &str) {
    open_startup_command_palette(harness);
    harness.send_text(command).unwrap_or_else(|err| {
        panic!("failed to type native startup history command {command:?}: {err}")
    });
    harness.send_enter().unwrap_or_else(|err| {
        panic!("failed to open native startup history command {command:?}: {err}")
    });
    let history_screen = harness
        .wait_for_text(marker, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| {
            panic!("native startup history marker {marker:?} did not appear: {err}")
        });
    assert!(history_screen.contains(marker));
}

fn show_operator_sidebar(harness: &GhosttyNativeHarness, marker: &str) {
    harness.send_key("Tab").unwrap_or_else(|err| {
        panic!("failed to focus transcript before opening native sidebar: {err}")
    });
    harness
        .send_key("i")
        .unwrap_or_else(|err| panic!("failed to toggle native operator sidebar: {err}"));
    let sidebar_screen = harness
        .wait_for_text(marker, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| {
            panic!("native operator sidebar marker {marker:?} did not appear: {err}")
        });
    assert!(sidebar_screen.contains(marker));
}

fn move_list_selection(harness: &GhosttyNativeHarness, steps: usize) {
    for _ in 0..steps {
        harness
            .send_key("Down")
            .unwrap_or_else(|err| panic!("failed to move native list selection: {err}"));
    }
}

fn run_session_palette_command(
    harness: &GhosttyNativeHarness,
    command_filter: &str,
    marker: &str,
) -> String {
    harness.send_key("Tab").unwrap_or_else(|err| {
        panic!("failed to move focus off prompt before palette command: {err}")
    });
    harness
        .send_key("C-p")
        .unwrap_or_else(|err| panic!("failed to open native session command palette: {err}"));
    harness
        .wait_for_text(STARTUP_COMMAND_PALETTE_MARKER, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native session command palette did not appear: {err}"));
    harness
        .send_text(command_filter)
        .unwrap_or_else(|err| panic!("failed to filter native session palette command: {err}"));
    harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to run native session palette command: {err}"));
    harness
        .wait_for_text(marker, STARTUP_TIMEOUT)
        .unwrap_or_else(|err| panic!("native session palette command marker did not appear: {err}"))
}

fn seed_completed_mock_session(
    repo_root: &Path,
    harness_bin: &Path,
    session_dir: &Path,
    prompt_text: &str,
) {
    let harness =
        GhosttyNativeHarness::spawn_mock(harness_bin, repo_root, session_dir, PRIMARY_GRID.clone())
            .unwrap_or_else(|err| panic!("failed to spawn native seed harness: {err}"));
    wait_for_startup_ready(&harness, STARTUP_HOME_SHORTCUT_MARKER);
    harness
        .send_text(prompt_text)
        .unwrap_or_else(|err| panic!("failed to type native seed prompt: {err}"));
    harness
        .send_enter()
        .unwrap_or_else(|err| panic!("failed to submit native seed prompt: {err}"));
    let finished_screen = harness
        .wait_for_all_text(RUN_FINISHED_SHELL_MARKERS, RUN_TIMEOUT)
        .unwrap_or_else(|err| panic!("native seed session did not finish: {err}"));
    assert!(finished_screen.contains(LIVE_READY_NEXT_TURN_MARKER));
    harness
        .cleanup()
        .unwrap_or_else(|err| panic!("failed to clean up native seed harness: {err}"));
}

fn new_native_visual_run(
    test_name: &str,
    grid: &NativeVisualGrid,
    extra_metadata: Value,
) -> LiveVisualRun {
    LiveVisualRun::new_with_options(
        test_name,
        &run_id(),
        LiveVisualRunOptions {
            run_metadata: merge_metadata(
                default_native_visual_run_metadata(test_name, grid),
                extra_metadata,
            ),
            artifact_namespace: "native-visual".to_string(),
            png_prefix: "native_visual".to_string(),
        },
    )
    .expect("create native visual manifest-backed run")
}

fn finalize_native_visual_run(harness: GhosttyNativeHarness, visual_run: &LiveVisualRun) {
    let summary = harness
        .cleanup()
        .unwrap_or_else(|err| panic!("failed to clean up Ghostty native visual session: {err}"));
    write_native_visual_summary(visual_run.run_dir(), &summary)
        .unwrap_or_else(|err| panic!("failed to write native visual summary: {err}"));
}

fn merge_metadata(base: Value, extra: Value) -> Value {
    match (base, extra) {
        (Value::Object(mut base), Value::Object(extra)) => {
            for (key, value) in extra {
                base.insert(key, value);
            }
            Value::Object(base)
        }
        (value, _) => value,
    }
}

fn run_native_visual_smoke_in_managed_session(test_name: &str) -> Result<(), String> {
    let test_binary = env::current_exe()
        .map_err(|err| format!("failed to resolve native visual test binary path: {err}"))?;
    let parent_session = resolve_parent_graphical_session_env()?;
    let socket_name = format!("wayland-native-visual-{}", std::process::id());
    let driver_script = write_managed_session_driver(&test_binary, test_name)?;
    let mut command = Command::new("dbus-run-session");
    parent_session.apply_to_command(&mut command);
    command
        .env("HARNESS_NATIVE_VISUAL", "1")
        .env(NATIVE_VISUAL_INNER_ENV, "1")
        .env("KWIN_SCREENSHOT_NO_PERMISSION_CHECKS", "1")
        .env("NO_AT_BRIDGE", "1")
        .env("GTK_USE_PORTAL", "0")
        .arg("--")
        .arg("kwin_wayland")
        .arg("--xwayland")
        .arg("--no-lockscreen")
        .arg("--no-global-shortcuts")
        .arg("--socket")
        .arg(&socket_name)
        .arg("--width")
        .arg(NATIVE_VISUAL_SESSION_WIDTH.to_string())
        .arg("--height")
        .arg(NATIVE_VISUAL_SESSION_HEIGHT.to_string());
    parent_session.append_windowed_backend_args(&mut command)?;
    let output = command
        .arg("--exit-with-session")
        .arg(&driver_script)
        .output()
        .map_err(|err| format!("failed to launch managed nested KWin session: {err}"))?;
    let _ = fs::remove_file(&driver_script);
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "managed nested native visual session failed: status={} stdout=`{}` stderr=`{}`",
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim(),
    ))
}

fn write_managed_session_driver(test_binary: &Path, test_name: &str) -> Result<PathBuf, String> {
    let script_dir = unique_temp_dir("native-visual-session-driver");
    fs::create_dir_all(&script_dir).map_err(|err| {
        format!(
            "failed to create managed native visual driver dir {}: {err}",
            script_dir.display()
        )
    })?;
    let script_path = script_dir.join("run-inner-native-visual.sh");
    let script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nexec {} {} --ignored --exact\n",
        shell_escape_text(&test_binary.to_string_lossy()),
        shell_escape_text(test_name),
    );
    fs::write(&script_path, script).map_err(|err| {
        format!(
            "failed to write managed native visual driver {}: {err}",
            script_path.display()
        )
    })?;
    let mut permissions = fs::metadata(&script_path)
        .map_err(|err| {
            format!(
                "failed to stat managed native visual driver {}: {err}",
                script_path.display()
            )
        })?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script_path, permissions).map_err(|err| {
        format!(
            "failed to mark managed native visual driver {} executable: {err}",
            script_path.display()
        )
    })?;
    Ok(script_path)
}

fn resolve_harness_bin() -> PathBuf {
    static HARNESS_BIN_CACHE: OnceLock<PathBuf> = OnceLock::new();
    HARNESS_BIN_CACHE
        .get_or_init(|| {
            if let Ok(path) = env::var("HARNESS_BIN") {
                let harness_bin = PathBuf::from(path);
                assert!(
                    harness_bin.exists(),
                    "HARNESS_BIN points to missing path: {}",
                    harness_bin.display()
                );
                return harness_bin;
            }

            if let Some(path) = option_env!("CARGO_BIN_EXE_harness") {
                let harness_bin = PathBuf::from(path);
                if harness_bin.exists() {
                    return harness_bin;
                }
            }

            let repo = repo_root();
            let harness_bin = repo
                .join("target")
                .join("debug")
                .join(binary_name("harness"));
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
            assert!(
                harness_bin.exists(),
                "expected harness binary at {}",
                harness_bin.display()
            );
            harness_bin
        })
        .clone()
}

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .expect("harness-testkit should live under <repo>/crates/harness-testkit")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let base = env::temp_dir().join("harness-testkit");
    fs::create_dir_all(&base).expect("create base temp dir");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    let dir = base.join(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("create unique temp dir");
    dir
}

fn run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("run-{}-{nanos}", std::process::id())
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
                    "tool_id": "fs.read",
                    "args_summary": "{\"path\":\"src/ui.rs\",\"offset\":12,\"limit\":24}",
                    "args_digest": "digest-native-read-args",
                    "metadata": {
                        "canonical_tool_id": "fs.read",
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
                        "canonical_tool_id": "fs.read",
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
                        "canonical_tool_id": "fs.read",
                        "alias_source_tool_id": "read",
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
                        "canonical_tool_id": "fs.read",
                        "alias_source_tool_id": "read",
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
                        "canonical_tool_id": "agent.spawn",
                        "alias_source_tool_id": "task",
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
                        "canonical_tool_id": "agent.spawn",
                        "alias_source_tool_id": "task",
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
                        "canonical_tool_id": "web.fetch",
                        "alias_source_tool_id": "webfetch",
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
                        "canonical_tool_id": "web.fetch",
                        "alias_source_tool_id": "webfetch",
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
                    "tool_id": "shell.run",
                    "args_summary": "{\"cmd\":\"cargo test -p harness-tui\",\"cwd\":\"/workspace\"}",
                    "args_digest": "digest-shell-fail-args",
                    "metadata": {
                        "canonical_tool_id": "shell.run",
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
                        "canonical_tool_id": "shell.run",
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
    fs::create_dir_all(run_dir.join("artifacts"))
        .expect("create native corrupt fixture artifacts dir");
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
    .expect("serialize native corrupt fixture first line");
    fs::write(
        run_dir.join("events.jsonl"),
        format!("{valid_first_line}\n{{bad-json}}\n"),
    )
    .expect("write native corrupt fixture events");
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
    .expect("write native replay fixture metadata");
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

fn write_session_fixture(session_dir: &Path, run_id: &str, events: &[Value]) -> PathBuf {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(run_dir.join("artifacts"))
        .expect("create native visual fixture artifacts dir");

    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("serialize native visual fixture event");
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(run_dir.join("events.jsonl"), body).expect("write native visual fixture events");
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

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

fn shell_escape_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, Default)]
struct ParentGraphicalSessionEnv {
    display: Option<String>,
    wayland_display: Option<String>,
    xauthority: Option<String>,
    xdg_runtime_dir: Option<String>,
}

impl ParentGraphicalSessionEnv {
    fn from_process_env() -> Self {
        Self {
            display: non_empty_env("DISPLAY"),
            wayland_display: non_empty_env("WAYLAND_DISPLAY"),
            xauthority: non_empty_env("XAUTHORITY"),
            xdg_runtime_dir: non_empty_env("XDG_RUNTIME_DIR"),
        }
    }

    fn merge_missing_from(&mut self, other: Self) {
        if self.display.is_none() {
            self.display = other.display;
        }
        if self.wayland_display.is_none() {
            self.wayland_display = other.wayland_display;
        }
        if self.xauthority.is_none() {
            self.xauthority = other.xauthority;
        }
        if self.xdg_runtime_dir.is_none() {
            self.xdg_runtime_dir = other.xdg_runtime_dir;
        }
    }

    fn apply_to_command(&self, command: &mut Command) {
        if let Some(value) = &self.display {
            command.env("DISPLAY", value);
        }
        if let Some(value) = &self.wayland_display {
            command.env("WAYLAND_DISPLAY", value);
        }
        if let Some(value) = &self.xauthority {
            command.env("XAUTHORITY", value);
        }
        if let Some(value) = &self.xdg_runtime_dir {
            command.env("XDG_RUNTIME_DIR", value);
        }
    }

    fn append_windowed_backend_args(&self, command: &mut Command) -> Result<(), String> {
        if let Some(value) = &self.wayland_display {
            command.arg("--wayland-display").arg(value);
            return Ok(());
        }
        if let Some(value) = &self.display {
            command.arg("--x11-display").arg(value);
            return Ok(());
        }
        Err(
            "native visual nested KWin session requires either WAYLAND_DISPLAY or DISPLAY from the parent graphical session"
                .to_string(),
        )
    }
}

fn resolve_parent_graphical_session_env() -> Result<ParentGraphicalSessionEnv, String> {
    let mut resolved = ParentGraphicalSessionEnv::from_process_env();
    if resolved.display.is_some() || resolved.wayland_display.is_some() {
        return Ok(resolved);
    }

    resolved.merge_missing_from(read_process_environment(PLASMASHELL_PROCESS_NAME)?);
    if resolved.display.is_none() && resolved.wayland_display.is_none() {
        return Err(
            "failed to discover DISPLAY/WAYLAND_DISPLAY for the active graphical session; set DISPLAY or WAYLAND_DISPLAY before running HARNESS_NATIVE_VISUAL=1"
                .to_string(),
        );
    }
    Ok(resolved)
}

fn read_process_environment(process_name: &str) -> Result<ParentGraphicalSessionEnv, String> {
    let output = Command::new("ps")
        .arg("eww")
        .arg("-C")
        .arg(process_name)
        .output()
        .map_err(|err| {
            format!("failed to inspect process environment for {process_name}: {err}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "failed to inspect process environment for {process_name}: status={} stdout=`{}` stderr=`{}`",
            output.status,
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| format!("ps output for {process_name} was not valid UTF-8: {err}"))?;
    let env_line = stdout
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| format!("no running {process_name} process exposed a usable environment"))?;

    Ok(ParentGraphicalSessionEnv {
        display: env_token_value(env_line, "DISPLAY"),
        wayland_display: env_token_value(env_line, "WAYLAND_DISPLAY"),
        xauthority: env_token_value(env_line, "XAUTHORITY"),
        xdg_runtime_dir: env_token_value(env_line, "XDG_RUNTIME_DIR"),
    })
}

fn non_empty_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_token_value(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}
