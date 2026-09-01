use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent, RuntimeEvent,
    ToolCallRequestedEvent, SCHEMA_VERSION,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::cmp;
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

const MARKER_TIMEOUT: Duration = Duration::from_secs(12);
const EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const READ_POLL_TIMEOUT: Duration = Duration::from_millis(50);
const PTY_SIGNOFF_ENV: &str = "HARNESS_TUI_PTY_SIGNOFF";
const HELPER_SCENARIO_ENV: &str = "HARNESS_TUI_PTY_HELPER_SCENARIO";
const TYPE_FIRST_STARTUP_SCENARIO: &str = "type_first_startup";
const SCROLL_FOLLOW_SCENARIO: &str = "scroll_follow";
const CONNECT_AUTH_SCENARIO: &str = "connect_auth";
const PERMISSION_OVERLAY_SCENARIO: &str = "permission_overlay";
const TYPE_FIRST_STARTUP_TEST: &str = "pty_helper_type_first_startup";
const SCROLL_FOLLOW_TEST: &str = "pty_helper_scroll_follow";
const CONNECT_AUTH_TEST: &str = "pty_helper_connect_auth";
const PERMISSION_OVERLAY_TEST: &str = "pty_helper_permission_overlay";
const DRAFT_TEXT: &str = "Hello from PTY";
const PERMISSION_DRAFT: &str = "keep draft under permission";
const CLEAR_DRAFT_TEXT: &str = "draft to clear via esc";
const CLEAR_PROMPT_HINT: &str = "press again to clear";
const PERMISSION_INJECT_DELAY: Duration = Duration::from_millis(5_000);

const PRIMARY_COLS: u16 = 100;
const PRIMARY_ROWS: u16 = 30;
const MINIMUM_COLS: u16 = 80;
const MINIMUM_ROWS: u16 = 24;

pub(crate) fn pty_smoke_starts_accepts_input_resizes_and_exits() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for("❯");
    let startup_screen = helper.screen_text();
    assert_fresh_session_prompt_glyph_contract(&startup_screen);

    helper
        .writer
        .write_all(DRAFT_TEXT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(DRAFT_TEXT);

    helper
        .master
        .resize(pty_size(MINIMUM_COLS, MINIMUM_ROWS))
        .unwrap_or_abort();
    helper.parser = Parser::new(MINIMUM_ROWS, MINIMUM_COLS, 0);
    helper.wait_for("❯");

    send_key(helper.writer.as_mut(), 0x10).unwrap_or_abort();
    helper.wait_for("Commands");
    let palette_screen = helper.screen_text();
    assert_no_sidebar_copy(&palette_screen, "palette");
    helper.writer.write_all(b"exit the app").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("Exit the app");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    wait_for_child_exit(&mut helper, "pty_smoke_exit");
}

pub(crate) fn pty_scroll_follow_requires_second_clamped_page_down() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_scroll_follow_helper();
    helper.wait_for("中文双宽字符");
    helper
        .master
        .resize(pty_size(MINIMUM_COLS, MINIMUM_ROWS))
        .unwrap_or_abort();
    helper.parser = Parser::new(MINIMUM_ROWS, MINIMUM_COLS, 0);
    helper.wait_for("中文双宽字符");

    send_bytes(helper.writer.as_mut(), b"\x1b[5~").unwrap_or_abort();
    helper.wait_for("▼");
    let page_up_screen = helper.screen_text();
    let arrow_column = page_up_screen
        .lines()
        .find_map(|line| line.chars().position(|character| character == '▼'))
        .unwrap_or_abort();
    assert!(
        (38..=41).contains(&arrow_column),
        "detached indicator must stay centered at 80 columns\n{page_up_screen}"
    );

    send_bytes(helper.writer.as_mut(), b"\x1b[6~").unwrap_or_abort();
    helper.wait_until_absent("▼");
    let landed_screen = helper.screen_text();
    assert!(
        landed_screen.contains("中文双宽字符"),
        "first PageDown must keep the CJK transcript intact\n{landed_screen}"
    );

    send_bytes(helper.writer.as_mut(), b"\x1b[6~").unwrap_or_abort();
    send_key(helper.writer.as_mut(), 0x10).unwrap_or_abort();
    helper.wait_for("Commands");
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_until_absent("Commands");
    let following_screen = helper.screen_text();
    assert!(
        following_screen.contains("中文双宽字符") && !following_screen.contains('▼'),
        "second clamped PageDown must resume the live tail without CJK drift\n{following_screen}"
    );

    println!(
        "--- detached-page-up ---\n{page_up_screen}\n--- landed-first-page-down ---\n{landed_screen}\n--- following-second-page-down ---\n{following_screen}"
    );
    exit_via_palette(&mut helper);
}

pub(crate) fn pty_connect_auth_drives_provider_connection() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_helper(CONNECT_AUTH_TEST, CONNECT_AUTH_SCENARIO);
    helper.wait_for("❯");

    send_key(helper.writer.as_mut(), b'/').unwrap_or_abort();
    helper.writer.write_all(b"connect").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("connect");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();

    helper.wait_for("Connect a provider");
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_until_absent("Connect a provider");
    helper.wait_for("❯");
    helper.writer.write_all(b"/exit").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    wait_for_child_exit(&mut helper, "pty_connect_auth_exit");
}

pub(crate) fn pty_permission_overlay_resolves_and_preserves_draft() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_helper(PERMISSION_OVERLAY_TEST, PERMISSION_OVERLAY_SCENARIO);
    helper.wait_for("❯");

    helper
        .writer
        .write_all(PERMISSION_DRAFT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(PERMISSION_DRAFT);

    helper.wait_for("Allow Edit");
    helper.wait_for("always-approve");
    let permission_screen = helper.screen_text();
    assert_permission_dock_shell(&permission_screen);
    assert!(
        permission_screen.contains("Allow Edit to demo.txt")
            || permission_screen.contains("Apply hashline edit to demo.txt"),
        "PTY permission dock must show edit target or summary\n{permission_screen}"
    );
    assert!(
        permission_screen.contains(PERMISSION_DRAFT),
        "PTY permission dock must preserve composer draft\n{permission_screen}"
    );
    assert!(
        permission_screen.contains('●') || permission_screen.contains("(●)"),
        "PTY permission dock must show selected radio marker\n{permission_screen}"
    );

    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    helper.wait_for("Cancel");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    helper.wait_until_absent("Allow Edit");
    let after_resolve = helper.screen_text();
    assert!(
        !after_resolve.contains("Allow Edit"),
        "permission dock must clear after confirmed allow + resolved event\n{after_resolve}"
    );
    assert!(
        after_resolve.contains(PERMISSION_DRAFT),
        "draft must remain after permission resolve\n{after_resolve}"
    );
    exit_via_palette(&mut helper);
}

pub(crate) fn pty_status_dialog_opens_without_sidebar_copy() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for("❯");

    send_key(helper.writer.as_mut(), 0x18).unwrap_or_abort();
    send_key(helper.writer.as_mut(), b's').unwrap_or_abort();
    helper.wait_for("Status · Harness dashboard");
    helper.wait_for("No MCP Servers");
    let leader_status = helper.screen_text();
    assert!(
        leader_status.contains("Status · Harness dashboard") && !leader_status.contains("Commands"),
        "PTY status dialog must own its title without Commands palette chrome\n{leader_status}"
    );
    assert_no_sidebar_copy(&leader_status, "status dialog via Ctrl+x s");
    assert!(
        leader_status.contains("MCP")
            || leader_status.contains("LSP")
            || leader_status.contains("No MCP")
            || leader_status.contains("Plugins"),
        "PTY status dialog must show operator status content\n{leader_status}"
    );
    assert_dashboard_full_surface(&leader_status, PRIMARY_COLS, PRIMARY_ROWS);

    helper
        .master
        .resize(pty_size(MINIMUM_COLS, MINIMUM_ROWS))
        .unwrap_or_abort();
    helper.parser = Parser::new(MINIMUM_ROWS, MINIMUM_COLS, 0);
    helper.wait_for("Status · Harness dashboard");
    helper.wait_for("No MCP Servers");
    let compact_status = helper.screen_text();
    assert_dashboard_full_surface(&compact_status, MINIMUM_COLS, MINIMUM_ROWS);

    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_until_absent("Status · Harness dashboard");

    send_key(helper.writer.as_mut(), 0x10).unwrap_or_abort();
    helper.wait_for("Commands");
    helper.writer.write_all(b"Open status").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("Open status");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    helper.wait_for("Status · Harness dashboard");
    helper.wait_for("No MCP Servers");
    let palette_status = helper.screen_text();
    assert_no_sidebar_copy(&palette_status, "status dialog via palette");
    assert_dashboard_full_surface(&palette_status, MINIMUM_COLS, MINIMUM_ROWS);

    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_until_absent("Status · Harness dashboard");
    exit_via_palette(&mut helper);
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
pub(crate) fn pty_draft_esc_esc_clears_composer() {
    if !cfg!(target_os = "linux") || std::env::var(PTY_SIGNOFF_ENV).as_deref() != Ok("1") {
        return;
    }

    // Busy-turn Ctrl+C needs a live TaskScheduled stream; helpers do not drive a
    // provider. Continuous-use cancel-adjacent path: Esc Esc clears draft.
    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for("❯");

    helper
        .writer
        .write_all(CLEAR_DRAFT_TEXT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(CLEAR_DRAFT_TEXT);

    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_for(CLEAR_PROMPT_HINT);
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();

    let deadline = Instant::now() + MARKER_TIMEOUT;
    loop {
        let screen = helper.screen_text();
        if !screen.contains(CLEAR_DRAFT_TEXT) {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Esc Esc must clear composer draft within {MARKER_TIMEOUT:?}\n{screen}");
        }
        thread::sleep(READ_POLL_TIMEOUT);
    }

    exit_via_palette(&mut helper);
}

pub(crate) fn pty_helper_type_first_startup() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(TYPE_FIRST_STARTUP_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (_keepalive, update_rx) = live_update_channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_scroll_follow() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(SCROLL_FOLLOW_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let config = super::capture_events::scenario("responding").unwrap_or_abort();
    let (_keepalive, update_rx) = live_update_channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: config.events,
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_permission_overlay() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(PERMISSION_OVERLAY_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = live_update_channel();
    let keepalive_tx = update_tx.clone();
    let inject_tx = update_tx.clone();
    thread::spawn(move || {
        thread::sleep(PERMISSION_INJECT_DELAY);
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
            Box::new(permission_requested_event(
                2,
                "perm_pty_overlay",
                "tool_call_pty_overlay",
            )),
        ))));
        thread::sleep(Duration::from_millis(500));
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
            Box::new(permission_requested_event(
                2,
                "perm_pty_overlay",
                "tool_call_pty_overlay",
            )),
        ))));
    });

    let resolve_tx = update_tx.clone();
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        if let UiIntent::ResolvePermission {
            permission_id,
            decision,
            reason,
            ..
        } = intent
        {
            let event_decision = match decision {
                harness_core::perm::PermissionDecision::Allow => {
                    harness_core::event::PermissionDecision::Allow
                }
                harness_core::perm::PermissionDecision::Deny => {
                    harness_core::event::PermissionDecision::Deny
                }
            };
            let _ = resolve_tx.send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
                Box::new(permission_resolved_event(
                    3,
                    &permission_id,
                    event_decision,
                    reason,
                )),
            ))));
        }
    });

    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: vec![permission_seed_tool_call_event()],
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: Some(on_ui_intent),
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
    drop(keepalive_tx);
}

pub(crate) fn pty_helper_connect_auth() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(CONNECT_AUTH_SCENARIO) {
        return;
    }

    let _workspace_root = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = live_update_channel();
    let auth_tx = update_tx.clone();
    let on_ui_intent: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(move |intent| {
        if matches!(intent, UiIntent::OpenAuthManager { .. }) {
            auth_tx
                .send(LiveUpdate::AuthBackendResult {
                    success: true,
                    message: "authenticated".to_string(),
                })
                .unwrap_or_abort();
        }
    });

    run_tui_with_options(TuiOptions {
        mode: TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
        },
        exit_on_finish: false,
        on_ui_intent: Some(on_ui_intent),
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

struct SpawnedHelper {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    parser: Parser,
}

impl SpawnedHelper {
    fn wait_for(&mut self, needle: &str) {
        wait_for_screen_contains(&mut self.parser, &self.output_rx, needle);
    }

    fn wait_until_absent(&mut self, needle: &str) {
        wait_for_screen_absent(&mut self.parser, &self.output_rx, needle);
    }

    fn screen_text(&mut self) -> String {
        drain_output(&mut self.parser, &self.output_rx);
        self.parser.screen().contents()
    }
}

fn spawn_type_first_startup_helper() -> SpawnedHelper {
    spawn_helper(TYPE_FIRST_STARTUP_TEST, TYPE_FIRST_STARTUP_SCENARIO)
}

fn spawn_scroll_follow_helper() -> SpawnedHelper {
    spawn_helper(SCROLL_FOLLOW_TEST, SCROLL_FOLLOW_SCENARIO)
}

fn exit_via_palette(helper: &mut SpawnedHelper) {
    send_key(helper.writer.as_mut(), 0x10).unwrap_or_abort();
    helper.wait_for("Commands");
    helper.writer.write_all(b"exit the app").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("Exit the app");
    send_key(helper.writer.as_mut(), b'\r').unwrap_or_abort();
    wait_for_child_exit(helper, "exit_via_palette");
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn wait_for_child_exit(helper: &mut SpawnedHelper, context: &str) {
    let deadline = Instant::now() + EXIT_TIMEOUT;
    loop {
        match helper.child.try_wait() {
            Ok(Some(status)) => {
                assert!(
                    status.success(),
                    "{context}: helper tui child exited with {status:?}"
                );
                return;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(READ_POLL_TIMEOUT);
            }
            Err(err) => panic!("{context}: try_wait failed: {err}"),
        }
    }
    let _ = helper.child.kill();
    let _ = helper.child.wait();
}

fn assert_fresh_session_prompt_glyph_contract(screen: &str) {
    let prompt_glyph_lines = screen.lines().filter(|line| line.contains('❯')).count();
    assert_eq!(
        prompt_glyph_lines, 4,
        "PTY fresh session must paint three example prompts and one composer prompt (found {prompt_glyph_lines})\n{screen}"
    );
    let example_prompts = [
        "inspect src/ui.rs",
        "trace the failing test",
        "review the latest edit",
    ];
    for copy in example_prompts
        .into_iter()
        .chain(["Ask Harness to inspect, edit, or explain…"])
    {
        assert!(
            screen.contains(copy),
            "PTY fresh session must show `{copy}`\n{screen}"
        );
    }
    let example_columns = example_prompts.map(|copy| {
        screen
            .lines()
            .find(|line| line.contains(copy))
            .and_then(|line| line.find('❯'))
            .unwrap_or_abort()
    });
    assert!(
        example_columns
            .windows(2)
            .all(|columns| columns[0] == columns[1]),
        "PTY fresh-session example prompts must share one left edge: {example_columns:?}\n{screen}"
    );
    assert!(
        !screen.contains('┃'),
        "PTY fresh session must not render legacy composer rail ┃\n{screen}"
    );
}

fn assert_permission_dock_shell(screen: &str) {
    let prompt_glyph_lines = screen.lines().filter(|line| line.contains('❯')).count();
    assert!(
        prompt_glyph_lines <= 1,
        "PTY permission overlay must not paint a multi-row ❯ rail (found {prompt_glyph_lines})\n{screen}"
    );
    assert!(
        screen.contains('┃'),
        "PTY permission dock must paint product warning rail ┃\n{screen}"
    );
    assert!(
        screen.contains("Allow Edit"),
        "PTY permission dock must show Allow Edit title\n{screen}"
    );
}

fn assert_no_sidebar_copy(screen: &str, context: &str) {
    let lower = screen.to_ascii_lowercase();
    assert!(
        !lower.contains("show sidebar")
            && !lower.contains("hide sidebar")
            && !lower.contains("operator sidebar"),
        "PTY {context} must not advertise sidebar chrome copy\n{screen}"
    );
}

fn assert_dashboard_full_surface(screen: &str, cols: u16, rows: u16) {
    let lines = screen.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        usize::from(rows),
        "PTY dashboard must paint every terminal row\n{screen}"
    );
    assert!(
        lines
            .first()
            .is_some_and(|line| line.starts_with("┌─ Status · Harness dashboard")),
        "PTY dashboard title border must start at the first terminal cell\n{screen}"
    );
    let bottom = lines.last().unwrap_or_abort();
    assert!(
        bottom.starts_with('└') && bottom.ends_with('┘'),
        "PTY dashboard bottom border must own the final terminal row\n{screen}"
    );
    assert_eq!(
        bottom.chars().count(),
        usize::from(cols),
        "PTY dashboard bottom border must span the terminal width\n{screen}"
    );
    assert!(
        !screen.contains("Ask Harness to inspect, edit, or explain"),
        "PTY dashboard must replace, not overlay, the session surface\n{screen}"
    );
}

fn permission_seed_tool_call_event() -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt_pty_perm_tool_0001".to_string(),
        seq: 1,
        run_id: "run_pty_permission_overlay".into(),
        mono_ms: 100,
        ts: Some("2026-07-17T12:00:00Z".to_string()),
        actor: EventActor::new(
            ActorKind::System,
            Some("pty-permission-overlay".to_string()),
        ),
        correlation_id: Some("req_pty_perm".to_string()),
        causation_id: None,
        stream_key: None,
        payload: EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool_call_pty_overlay".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"demo.txt"}"#.to_string(),
            args_digest: "digest-args-pty-perm".to_string(),
            metadata: None,
        }),
    }
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_pty_perm_{seq:04}"),
        seq,
        run_id: "run_pty_permission_overlay".into(),
        mono_ms: 400,
        ts: Some("2026-07-17T12:00:00Z".to_string()),
        actor: EventActor::new(
            ActorKind::System,
            Some("pty-permission-overlay".to_string()),
        ),
        correlation_id: Some(permission_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: format!("digest-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    }
}

fn permission_resolved_event(
    seq: u64,
    permission_id: &str,
    decision: harness_core::event::PermissionDecision,
    reason: Option<String>,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_pty_perm_resolved_{seq:04}"),
        seq,
        run_id: "run_pty_permission_overlay".into(),
        mono_ms: seq,
        ts: Some("2026-07-17T12:00:01Z".to_string()),
        actor: EventActor::new(
            ActorKind::System,
            Some("pty-permission-overlay".to_string()),
        ),
        correlation_id: Some(permission_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload: EventV1::PermissionResolved(harness_core::event::PermissionResolvedEvent {
            permission_id: permission_id.to_string(),
            decision,
            reason,
        }),
    }
}

fn spawn_helper(test_name: &str, scenario: &str) -> SpawnedHelper {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size(PRIMARY_COLS, PRIMARY_ROWS))
        .unwrap_or_abort();

    let current_test_bin = std::env::current_exe().unwrap_or_abort();
    let mut command = CommandBuilder::new(current_test_bin.to_string_lossy().as_ref());
    command.arg("--exact");
    command.arg(test_name);
    command.arg("--nocapture");
    command.env(HELPER_SCENARIO_ENV, scenario);
    configure_deterministic_env(&mut command);

    let child = pair.slave.spawn_command(command).unwrap_or_abort();
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().unwrap_or_abort();
    let writer = pair.master.take_writer().unwrap_or_abort();
    let output_rx = spawn_reader_thread(reader);

    SpawnedHelper {
        master: pair.master,
        child,
        writer,
        output_rx,
        parser: Parser::new(PRIMARY_ROWS, PRIMARY_COLS, 0),
    }
}

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn wait_for_screen_contains(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, needle: &str) {
    let deadline = Instant::now() + MARKER_TIMEOUT;

    loop {
        drain_output(parser, output_rx);
        let current = parser.screen().contents();
        if current.contains(needle) {
            return;
        }

        let now = Instant::now();
        if now >= deadline {
            panic!(
                "PTY wait_for timed out after {MARKER_TIMEOUT:?} waiting for {needle:?}\n{current}"
            );
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        if let Ok(chunk) = output_rx.recv_timeout(wait_timeout) {
            parser.process(&chunk);
        }
    }
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
fn wait_for_screen_absent(parser: &mut Parser, output_rx: &Receiver<Vec<u8>>, needle: &str) {
    let deadline = Instant::now() + MARKER_TIMEOUT;

    loop {
        drain_output(parser, output_rx);
        let current = parser.screen().contents();
        if !current.contains(needle) {
            return;
        }

        let now = Instant::now();
        if now >= deadline {
            panic!(
                "PTY wait_for timed out after {MARKER_TIMEOUT:?} waiting for {needle:?} to disappear\n{current}"
            );
        }

        let wait_timeout = cmp::min(READ_POLL_TIMEOUT, deadline.saturating_duration_since(now));
        if let Ok(chunk) = output_rx.recv_timeout(wait_timeout) {
            parser.process(&chunk);
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

fn send_bytes(writer: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 || tx.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
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
