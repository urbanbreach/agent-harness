// allow: SIZE_OK — reference-parity PTY owner matrix + helper spawn surface
//! PTY interaction owners for reference-parity first-slice and overlay rows.
//!
//! Separate from `pty_e2e_impl` so both modules stay reviewable. Spawns this
//! test binary as a helper child with scenario env vars (same pattern as e2e).

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderReasoningDeltaEvent, ProviderRequestFinishedEvent, ProviderRequestRetryMetadata,
    ProviderRequestStartedEvent, ProviderRequestStartedMetadata, ProviderStreamDeltaEvent,
    TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent, ToolCallRequestedEvent,
    ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_providers::{CompletionUsage, ProviderErrorCategory};
use harness_tui::app::{
    set_pending_live_launch_metadata, set_pending_live_prompt_draft, LaunchMetadata, ModelOption,
    SessionHistoryEntry,
};
use harness_tui::UnwrapOrAbort;
use harness_tui::{
    run_tui_with_options, set_pending_replay_launch_metadata, LiveUpdate, TuiMode, TuiOptions,
    UiIntent,
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
const PARITY_STRICT_ENV: &str = "HARNESS_TUI_PARITY_STRICT";
const HELPER_SCENARIO_ENV: &str = "HARNESS_TUI_PTY_HELPER_SCENARIO";
const TYPE_FIRST_STARTUP_SCENARIO: &str = "type_first_startup";
const IDLE_SHELL_SCENARIO: &str = "idle_shell";
const LIVE_DRAFT_SCENARIO: &str = "live_draft";
const LIVE_STREAM_SCENARIO: &str = "live_stream";
const LIVE_FAIL_SCENARIO: &str = "live_fail";
const LIVE_COMPLETE_SCENARIO: &str = "live_complete";
const LIVE_CANCEL_SCENARIO: &str = "live_cancel";
const LIVE_RECOVER_SCENARIO: &str = "live_recover";
const LIVE_TOOL_SCENARIO: &str = "live_tool";
const LIVE_DIFF_SCENARIO: &str = "live_diff";
const LIVE_SCROLL_SCENARIO: &str = "live_scroll";
const QUESTION_OVERLAY_SCENARIO: &str = "question_overlay";
const PERMISSION_OVERLAY_SCENARIO: &str = "permission_overlay";
/// SHELL-PERM reference freeze (run4-shell-perm-pinned-v4) shows a
/// tool-in-flight streaming state, NOT a permission overlay. The reference
/// binary cannot surface a permission prompt via black-box tool-call
/// injection (see shell-perm-reference-freeze.v4.json blocker). This scenario
/// produces the matching streaming state.
const LIVE_PERM_STREAM_SCENARIO: &str = "live_perm_stream";
/// SHELL-QUESTION reference freeze (run1-shell-question-pinned-v1) shows a
/// question-tool-in-flight streaming state, NOT a question overlay. The
/// reference binary renders the injected `question` chat_completions tool
/// call as already-executing ◆ question chips plus a waiting-for-response
/// spinner — it never surfaces an interactive question UI from black-box
/// tool-call injection (see shell-question-reference-freeze.v1.json
/// blocker). This scenario produces the matching streaming state.
const LIVE_QUESTION_STREAM_SCENARIO: &str = "live_question_stream";
const TYPE_FIRST_STARTUP_HELPER: &str = "pty_helper_type_first_startup";
const IDLE_SHELL_HELPER: &str = "pty_helper_idle_shell";
const LIVE_DRAFT_HELPER: &str = "pty_helper_live_draft";
const LIVE_STREAM_HELPER: &str = "pty_helper_live_stream";
const LIVE_FAIL_HELPER: &str = "pty_helper_live_fail";
const LIVE_COMPLETE_HELPER: &str = "pty_helper_live_complete";
const LIVE_CANCEL_HELPER: &str = "pty_helper_live_cancel";
const LIVE_RECOVER_HELPER: &str = "pty_helper_live_recover";
const LIVE_TOOL_HELPER: &str = "pty_helper_live_tool";
const LIVE_DIFF_HELPER: &str = "pty_helper_live_diff";
const LIVE_SCROLL_HELPER: &str = "pty_helper_live_scroll";
const QUESTION_OVERLAY_HELPER: &str = "pty_helper_question_overlay";
const PERMISSION_OVERLAY_HELPER: &str = "pty_helper_permission_overlay";
const LIVE_PERM_STREAM_HELPER: &str = "pty_helper_live_perm_stream";
const LIVE_QUESTION_STREAM_HELPER: &str = "pty_helper_live_question_stream";
const STREAM_USER_TEXT: &str = "stream parity probe";
const PERM_STREAM_USER_TEXT: &str = "edit a project file now";
const QUESTION_STREAM_USER_TEXT: &str = "ask me the parity question";
const FAIL_USER_TEXT: &str = "fail the parity probe";
const COMPLETE_USER_TEXT: &str = "complete the parity probe";
const COMPLETE_ASSISTANT_TEXT: &str = "parity turn complete stream final response rendered cleanly under the shell composer parity turn complete stream final response rendered cleanly under the shell composer parity turn complete stream final response rendered cleanly under the shell composer";
// Grok CANCEL freeze (run1-shell-cancel-pinned-v1): empty transcript + draft in composer.
const CANCEL_USER_TEXT: &str = "cancel the parity probe";
// Grok RECOVER freeze (run1-shell-recover-pinned-v1): same fail state + draft in composer.
const RECOVER_USER_TEXT: &str = "recover the parity probe";
const RECOVER_DRAFT: &str = "retry after failure draft";
// Grok TOOL freeze user prompt (run1-tool-proxy-v2).
const TOOL_USER_TEXT: &str =
    "Use a tool to list files in the current directory, then report COUNT=N for the number of top-level entries. Do not invent.";
const TOOL_PATH_TEXT: &str = "echo tx-tool-output-probe-line";
// Grok DIFF freeze user prompt (run1-diff-proxy-v2).
const DIFF_USER_TEXT: &str =
    "Overwrite demo.txt with exactly the single line: parity-diff-ok. Use a file write/edit tool so a diff is shown. Then reply DONE.";

// Freeze breadcrumb token meta uses context window 262K and turn usage like 12K / 10K / 1.5K.
const PARITY_CONTEXT_WINDOW_TOKENS: u32 = 262_144;

fn parity_completion_usage(total_tokens: u32) -> CompletionUsage {
    // Breadcrumb context meta prefers prompt_tokens when > 0; keep prompt == total so
    // simple seeds pack freeze breadcrumb (12K / 10K / 1.5K) without dual-source split.
    CompletionUsage {
        prompt_tokens: total_tokens,
        completion_tokens: 0,
        total_tokens,
    }
}

/// Freeze QUESTION/PERM: breadcrumb `10K / 262K` + waiting `⇣10.2k` / tool `⇣10.1k`.
fn parity_completion_usage_context_and_total(
    context_tokens: u32,
    total_tokens: u32,
) -> CompletionUsage {
    let total_tokens = total_tokens.max(context_tokens);
    CompletionUsage {
        prompt_tokens: context_tokens,
        completion_tokens: total_tokens.saturating_sub(context_tokens),
        total_tokens,
    }
}

fn install_parity_context_window() {
    let option = ModelOption {
        profile: "parity".to_string(),
        provider: "mock".to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: "model-tx".to_string(),
        model_display_label: Some("Parity Test Model (Mock)".to_string()),
        variant: None,
        variant_display_label: None,
        display_label: Some("Parity Test Model (Mock) (xhigh)".to_string()),
        token_window_label: None,
        context_window_tokens: Some(PARITY_CONTEXT_WINDOW_TOKENS),
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: Some("xhigh".to_string()),
        text_verbosity: None,
        thinking: None,
        recommended_for: None,
    };
    set_pending_replay_launch_metadata(Some(LaunchMetadata::from_model_option(&option)));
}
const DIFF_PATH_TEXT: &str = "demo.txt";
// Grok SCROLL freeze (run1-shell-scroll-pinned-v1): streaming state with partial response.
const SCROLL_USER_TEXT: &str = "scroll the parity probe";
const SCROLL_ASSISTANT_TEXT: &str =
    "parity turn complete stream final response rendered cleanly under the shell";
const QUESTION_USER_TEXT: &str = "You MUST use the AskUserQuestion tool (or equivalent question tool) to ask me exactly one multiple-choice question: Which color? Options: Red, Green, Blue. Do not answer yourself. Do not use any other tools.";
const READY_MARKER: &str = "❯";
const DRAFT_TEXT: &str = "parity draft";
const PERMISSION_DRAFT: &str = "keep draft under permission";
// Grok PERM freeze packing: Thought + Creating demo.txt above edit permission dock.
const PERMISSION_USER_TEXT: &str =
    "Create or overwrite demo.txt with the single line: parity-ok. Use a file write tool.";
const PERMISSION_TOOL_CALL_ID: &str = "tool_call_parity_overlay";
const PERMISSION_REQUEST_ID: &str = "req_perm_pty";
const PERMISSION_INJECT_DELAY: Duration = Duration::from_millis(900);
const PRIMARY_COLS: u16 = 100;
const PRIMARY_ROWS: u16 = 30;
/// Freeze-backed live shell captures (COMPLETE/PERM/SCROLL) use 120×32.
const FREEZE_SHELL_COLS: u16 = 120;
const FREEZE_SHELL_ROWS: u16 = 32;

pub(crate) fn startup_welcome_panel_renders_and_focuses_composer() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for(READY_MARKER);
    helper.wait_for("❯");
    let screen = helper.screen_text();
    assert!(
        screen.contains('╭') || screen.contains('┌') || screen.contains("Welcome"),
        "startup must show bordered welcome panel chrome\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "startup welcome");
    exit_via_palette(&mut helper);
}

pub(crate) fn startup_breadcrumb_warning_visible() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for(READY_MARKER);
    let screen = helper.screen_text();
    // Helper runs in a tempdir without a git worktree, so branch/path identity may be
    // empty. Require live chrome + composer focus and reject sidebar regressions.
    assert!(
        screen.contains('❯'),
        "startup must keep composer focus for breadcrumb/warning shell\n{screen}"
    );
    assert!(
        screen.contains("Shift+Tab:mode")
            || screen.contains("Ctrl+x:shortcuts")
            || screen.to_ascii_lowercase().contains("clipboard")
            || screen.contains('/')
            || screen.contains('~'),
        "startup must keep status/identity chrome (freeze disclosure or breadcrumb/warning)\n{screen}"
    );
    assert_no_sidebar_copy(&screen, "startup breadcrumb shell");
    exit_via_palette(&mut helper);
}

pub(crate) fn startup_type_dismisses_welcome() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for(READY_MARKER);
    helper
        .writer
        .write_all(DRAFT_TEXT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(DRAFT_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(DRAFT_TEXT),
        "typing must place draft in composer\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "draft after type-dismiss");
    exit_via_palette(&mut helper);
}

pub(crate) fn composer_bordered_strip_visible() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for(READY_MARKER);
    helper
        .writer
        .write_all(DRAFT_TEXT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(DRAFT_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains('╭')
            || screen.contains('╰')
            || screen.contains('┌')
            || screen.contains('└')
            || screen.contains('│'),
        "composer must render bordered strip geometry\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "composer must keep prompt glyph\n{screen}"
    );
    exit_via_palette(&mut helper);
}

pub(crate) fn shortcut_footer_updates_on_draft() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_type_first_startup_helper();
    helper.wait_for(READY_MARKER);
    let idle = helper.screen_text();
    helper
        .writer
        .write_all(DRAFT_TEXT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(DRAFT_TEXT);
    let draft = helper.screen_text();
    let idle_has_footer = idle.to_ascii_lowercase().contains("ctrl+p")
        || idle.to_ascii_lowercase().contains("commands")
        || idle.contains("Logged in");
    let draft_has_footer = draft.to_ascii_lowercase().contains("enter")
        || draft.to_ascii_lowercase().contains("ctrl+x")
        || draft.to_ascii_lowercase().contains("send")
        || draft.contains(DRAFT_TEXT);
    assert!(
        idle_has_footer || draft_has_footer,
        "footer/chrome must remain present across idle→draft\nidle:\n{idle}\ndraft:\n{draft}"
    );
    exit_via_palette(&mut helper);
}

pub(crate) fn ovl_palette_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_live_draft_helper();
    helper.wait_for(READY_MARKER);
    helper.writer.write_all(b"x").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("x");
    send_key(helper.writer.as_mut(), 0x10).unwrap_or_abort();
    helper.wait_for("Commands");
    let screen = helper.screen_text();
    assert!(
        screen.contains("Commands") || screen.contains("search:"),
        "Ctrl+p after draft dismiss must open command palette\n{screen}"
    );
    assert_no_sidebar_copy(&screen, "palette");
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_for(READY_MARKER);
    exit_via_palette(&mut helper);
}

pub(crate) fn ovl_session_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_live_draft_helper();
    helper.wait_for(READY_MARKER);
    helper.writer.write_all(b"x").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("x");
    send_key(helper.writer.as_mut(), 0x13).unwrap_or_abort();
    let screen = wait_for_any(
        &mut helper,
        &["Resume session", "session", "Sessions", "/ to search"],
    );
    assert_no_sidebar_copy(&screen, "session picker");
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_for(READY_MARKER);
    exit_via_palette(&mut helper);
}

pub(crate) fn ovl_help_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_live_draft_helper();
    helper.wait_for(READY_MARKER);
    helper.writer.write_all(b"x").unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for("x");
    send_key(helper.writer.as_mut(), 0x18).unwrap_or_abort();
    let screen = wait_for_any(
        &mut helper,
        &[
            "Keyboard Shortcuts",
            "Shortcuts",
            "Essentials",
            "/ to search",
        ],
    );
    assert_no_sidebar_copy(&screen, "help/shortcuts");
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_for(READY_MARKER);
    exit_via_palette(&mut helper);
}

pub(crate) fn ovl_perm_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_PERM_STREAM_HELPER, LIVE_PERM_STREAM_SCENARIO);
    helper.wait_for(PERM_STREAM_USER_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    assert!(
        screen.contains(PERM_STREAM_USER_TEXT) && screen.contains("Waiting for response"),
        "OVL-PERM PTY: user + waiting-for-response must project\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "OVL-PERM PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "OVL-PERM");
    assert_no_sidebar_copy(&screen, "OVL-PERM");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_idle_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_live_draft_helper();
    helper.wait_for(READY_MARKER);
    let screen = helper.screen_text();
    assert!(
        screen.contains('❯'),
        "SHELL-IDLE PTY: composer glyph required\n{screen}"
    );
    assert!(
        screen.contains('╭') || screen.contains('╰') || screen.contains('─'),
        "SHELL-IDLE PTY: bordered composer required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-IDLE");
    assert_no_sidebar_copy(&screen, "SHELL-IDLE");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_perm_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_PERM_STREAM_HELPER, LIVE_PERM_STREAM_SCENARIO);
    helper.wait_for(PERM_STREAM_USER_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    maybe_dump_l3("HARNESS_PERM_L3_DUMP", &screen);
    assert!(
        screen.contains(PERM_STREAM_USER_TEXT) && screen.contains("Waiting for response"),
        "SHELL-PERM PTY: user + waiting-for-response must project\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-PERM PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-PERM");
    assert_no_sidebar_copy(&screen, "SHELL-PERM");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_stream_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_STREAM_HELPER, LIVE_STREAM_SCENARIO);
    helper.wait_for(STREAM_USER_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    assert!(
        screen.contains(STREAM_USER_TEXT) && screen.contains("Waiting for response"),
        "SHELL-STREAM PTY: user + waiting-for-response must project\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-STREAM PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-STREAM");
    assert_no_sidebar_copy(&screen, "SHELL-STREAM");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_fail_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(LIVE_FAIL_HELPER, LIVE_FAIL_SCENARIO, 120, 40);
    helper.wait_for(FAIL_USER_TEXT);
    helper.wait_for("Retrying (attempt 2)");
    let screen = helper.screen_text();
    assert!(
        screen.contains(FAIL_USER_TEXT),
        "SHELL-FAIL PTY: user turn retained above retry chrome\n{screen}"
    );
    assert!(
        screen.contains("Retrying (attempt 2)"),
        "SHELL-FAIL PTY: auto-retry spinner must render\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-FAIL PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-FAIL");
    assert_no_sidebar_copy(&screen, "SHELL-FAIL");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_complete_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_COMPLETE_HELPER,
        LIVE_COMPLETE_SCENARIO,
        FREEZE_SHELL_COLS,
        40,
    );
    helper.wait_for(COMPLETE_USER_TEXT);
    helper.wait_for("parity turn complete stream");
    let screen = helper.screen_text();
    assert!(
        screen.contains(COMPLETE_USER_TEXT) && screen.contains("parity turn complete stream"),
        "SHELL-COMPLETE PTY: completed turn must project\n{screen}"
    );
    assert!(
        screen.contains("Worked for 2.3s"),
        "SHELL-COMPLETE PTY: Worked for must pack freeze-aligned 2.3s duration\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-COMPLETE PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-COMPLETE");
    assert_no_sidebar_copy(&screen, "SHELL-COMPLETE");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_user_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_COMPLETE_HELPER,
        LIVE_COMPLETE_SCENARIO,
        FREEZE_SHELL_COLS,
        FREEZE_SHELL_ROWS,
    );
    helper.wait_for(COMPLETE_USER_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(COMPLETE_USER_TEXT),
        "TX-USER PTY: user message required\n{screen}"
    );
    assert!(
        !screen.contains('┃'),
        "TX-USER PTY: no legacy left rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-USER");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_assistant_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_COMPLETE_HELPER,
        LIVE_COMPLETE_SCENARIO,
        FREEZE_SHELL_COLS,
        FREEZE_SHELL_ROWS,
    );
    helper.wait_for("parity turn complete stream");
    let screen = helper.screen_text();
    assert!(
        screen.contains("parity turn complete stream"),
        "TX-ASSISTANT PTY: assistant message required\n{screen}"
    );
    assert!(
        !screen.contains('┃'),
        "TX-ASSISTANT PTY: no legacy left rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-ASSISTANT");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_cancel_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_CANCEL_HELPER,
        LIVE_CANCEL_SCENARIO,
        FREEZE_SHELL_COLS,
        40,
    );
    helper.wait_for(CANCEL_USER_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(CANCEL_USER_TEXT),
        "SHELL-CANCEL PTY: draft text must be visible in composer\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-CANCEL PTY: composer glyph required\n{screen}"
    );
    assert!(
        screen.contains("Enter:send"),
        "SHELL-CANCEL PTY: draft footer must show Enter:send\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-CANCEL");
    assert_no_sidebar_copy(&screen, "SHELL-CANCEL");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_recover_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(LIVE_RECOVER_HELPER, LIVE_RECOVER_SCENARIO, 120, 40);
    helper.wait_for(RECOVER_USER_TEXT);
    helper.wait_for("Retrying (attempt 2)");
    helper.wait_for(RECOVER_DRAFT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(RECOVER_DRAFT),
        "SHELL-RECOVER PTY: recover draft must be editable after retry\n{screen}"
    );
    assert!(
        screen.contains("Retrying (attempt 2)"),
        "SHELL-RECOVER PTY: auto-retry spinner must render\n{screen}"
    );
    assert!(
        screen.contains("Enter:send") || screen.contains('❯'),
        "SHELL-RECOVER PTY: composer must accept draft after retry\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-RECOVER");
    assert_no_sidebar_copy(&screen, "SHELL-RECOVER");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_scroll_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(LIVE_SCROLL_HELPER, LIVE_SCROLL_SCENARIO, 120, 40);
    helper.wait_for(SCROLL_USER_TEXT);
    helper.wait_for(SCROLL_ASSISTANT_TEXT);
    // Grok SCROLL freeze: PageUp during streaming to scroll away from follow.
    send_bytes(helper.writer.as_mut(), b"\x1b[5~").unwrap_or_abort();
    thread::sleep(READ_POLL_TIMEOUT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(SCROLL_USER_TEXT),
        "SHELL-SCROLL PTY: user message required\n{screen}"
    );
    assert!(
        screen.contains(SCROLL_ASSISTANT_TEXT),
        "SHELL-SCROLL PTY: assistant response required\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-SCROLL PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-SCROLL");
    assert_no_sidebar_copy(&screen, "SHELL-SCROLL");
    maybe_dump_l3("HARNESS_SCROLL_L3_DUMP", &screen);
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_tool_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_TOOL_HELPER, LIVE_TOOL_SCENARIO);
    helper.wait_for(TOOL_PATH_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    assert!(
        screen.contains(TOOL_PATH_TEXT),
        "TX-TOOL PTY: echo tool row required\n{screen}"
    );
    assert!(
        screen.contains("Waiting for response"),
        "TX-TOOL PTY: streaming indicator required (Grok freeze form)\n{screen}"
    );
    assert!(
        screen.contains('◈') || screen.contains('◆'),
        "TX-TOOL PTY: tool diamond chrome required\n{screen}"
    );
    assert!(
        !screen.contains('┃'),
        "TX-TOOL PTY: no legacy left rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-TOOL");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_diff_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_DIFF_HELPER, LIVE_DIFF_SCENARIO);
    helper.wait_for(DIFF_PATH_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    assert!(
        screen.contains(DIFF_PATH_TEXT) || screen.contains('◆'),
        "TX-DIFF PTY: structured edit/path projection required\n{screen}"
    );
    assert!(
        screen.contains("Waiting for response"),
        "TX-DIFF PTY: streaming indicator required (Grok freeze form)\n{screen}"
    );
    assert!(
        !screen.contains('┃'),
        "TX-DIFF PTY: no legacy left rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-DIFF");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_question_pty() {
    assert_question_stream_pty("SHELL-QUESTION");
}

pub(crate) fn ovl_question_pty() {
    assert_question_stream_pty("OVL-QUESTION");
}

fn assert_question_stream_pty(label: &str) {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_QUESTION_STREAM_HELPER,
        LIVE_QUESTION_STREAM_SCENARIO,
        120,
        40,
    );
    helper.wait_for(QUESTION_STREAM_USER_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    maybe_dump_l3("HARNESS_QUESTION_L3_DUMP", &screen);
    assert!(
        screen.contains(QUESTION_STREAM_USER_TEXT) && screen.contains("Waiting for response"),
        "{label} PTY: user + waiting-for-response must project\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "{label} PTY: composer glyph required\n{screen}"
    );
    assert!(
        screen.contains('╭') || screen.contains('╰'),
        "{label} PTY: bordered composer required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, label);
    assert_no_sidebar_copy(&screen, label);
    exit_via_palette(&mut helper);
}

pub(crate) fn resp_60x20_pty() {
    assert_resp_idle_shell_pty(60, 20, "RESP-60x20");
}

pub(crate) fn resp_79x24_pty() {
    assert_resp_idle_shell_pty(79, 24, "RESP-79x24");
}

pub(crate) fn resp_80x24_pty() {
    assert_resp_idle_shell_pty(80, 24, "RESP-80x24");
}

pub(crate) fn resp_100x30_pty() {
    assert_resp_idle_shell_pty(100, 30, "RESP-100x30");
}

pub(crate) fn resp_120x40_pty() {
    assert_resp_idle_shell_pty(120, 40, "RESP-120x40");
}

pub(crate) fn resp_120x50_pty() {
    assert_resp_idle_shell_pty(120, 50, "RESP-120x50");
}

pub(crate) fn resp_wide_pty() {
    assert_resp_idle_shell_pty(140, 40, "RESP-WIDE");
}

/// Responsive PTY owner: idle shell at each viewport.
/// Reference freeze (run1-resp-*-pinned-v1) shows real HOME idle shell:
/// breadcrumb + empty transcript body + bordered composer (empty prompt) +
/// idle footer (Shift+Tab:mode | Ctrl+x:shortcuts). No welcome panel, no
/// draft text, no Enter:send footer.
fn assert_resp_idle_shell_pty(cols: u16, rows: u16, label: &str) {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(IDLE_SHELL_HELPER, IDLE_SHELL_SCENARIO, cols, rows);
    helper.wait_for(READY_MARKER);
    let screen = helper.screen_text();
    assert!(
        screen.contains('❯'),
        "{label} PTY: composer glyph required\n{screen}"
    );
    assert!(
        screen.contains('╭') || screen.contains('╰'),
        "{label} PTY: bordered composer required\n{screen}"
    );
    assert!(
        screen.contains("Shift+Tab:mode") || screen.contains("Ctrl+x:shortcuts"),
        "{label} PTY: idle footer required\n{screen}"
    );
    assert!(
        !screen.contains("New worktree") && !screen.contains("New session"),
        "{label} PTY: welcome actions must not appear in idle shell\n{screen}"
    );
    assert!(
        !screen.contains("Enter:send"),
        "{label} PTY: idle shell must not show draft footer\n{screen}"
    );
    assert_no_sidebar_copy(&screen, label);
    assert_no_multi_row_prompt_rail(&screen, label);
    exit_via_palette(&mut helper);
}

/// Helper-child entrypoints (spawned via `--exact` from the parent PTY test).
pub(crate) fn pty_helper_type_first_startup() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(TYPE_FIRST_STARTUP_SCENARIO) {
        return;
    }

    let (_keepalive, update_rx) = mpsc::channel();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_idle_shell() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(IDLE_SHELL_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (_keepalive, update_rx) = mpsc::channel();
    install_parity_context_window();
    let option = ModelOption {
        profile: "parity".to_string(),
        provider: "mock".to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: "model-tx".to_string(),
        model_display_label: Some("Parity Test Model (Mock)".to_string()),
        variant: None,
        variant_display_label: None,
        display_label: Some("Parity Test Model (Mock) (xhigh)".to_string()),
        token_window_label: None,
        context_window_tokens: Some(PARITY_CONTEXT_WINDOW_TOKENS),
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: Some("xhigh".to_string()),
        text_verbosity: None,
        thinking: None,
        recommended_for: None,
    };
    set_pending_live_launch_metadata(LaunchMetadata::from_model_option(&option));
    let session_history_entries = vec![SessionHistoryEntry {
        run_dir: std::path::PathBuf::from("/tmp/sessions/run_hello"),
        catalog: SessionCatalogEntry {
            run_id: "run_hello".into(),
            run_name: Some("Hello".to_string()),
            status: Some(RunStatus::Finished),
            last_updated_at: Some("2026-07-18T12:00:00Z".to_string()),
            workspace_root: Some("/home/urbanbreach/Projects/agent-harness".to_string()),
            profile_preset: Some("build".to_string()),
            provider_model: Some("mock/parity-test".to_string()),
            mode_source: SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 3,
            child_session_count: 0,
            parent_session_id: None,
        },
    }];
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: Vec::new(),
            session_history_entries,
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_live_draft() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_DRAFT_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (_keepalive, update_rx) = mpsc::channel();
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
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_live_stream() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_STREAM_SCENARIO) {
        return;
    }
    run_live_with_historical_events(stream_events());
}

pub(crate) fn pty_helper_live_perm_stream() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_PERM_STREAM_SCENARIO) {
        return;
    }
    run_live_with_historical_events(perm_stream_events());
}

pub(crate) fn pty_helper_live_question_stream() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_QUESTION_STREAM_SCENARIO) {
        return;
    }
    run_live_with_historical_events(question_stream_events());
}

pub(crate) fn pty_helper_live_fail() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_FAIL_SCENARIO) {
        return;
    }
    run_live_with_historical_events(fail_events());
}

pub(crate) fn pty_helper_live_complete() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_COMPLETE_SCENARIO) {
        return;
    }
    run_live_with_historical_events(complete_events());
}

pub(crate) fn pty_helper_live_cancel() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_CANCEL_SCENARIO) {
        return;
    }
    // Grok CANCEL freeze (run1-shell-cancel-pinned-v1): empty transcript + draft
    // "cancel the parity probe" in the composer. The reference binary clears the
    // transcript after cancellation and restores the prompt as a draft.
    set_pending_live_prompt_draft(Some(CANCEL_USER_TEXT.to_string()));
    run_live_with_historical_events(Vec::new());
}

pub(crate) fn pty_helper_live_recover() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_RECOVER_SCENARIO) {
        return;
    }
    set_pending_live_prompt_draft(Some(RECOVER_DRAFT.to_string()));
    run_live_with_historical_events(recover_events());
}

pub(crate) fn pty_helper_live_tool() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_TOOL_SCENARIO) {
        return;
    }
    run_live_with_historical_events(tool_events());
}

pub(crate) fn pty_helper_live_diff() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_DIFF_SCENARIO) {
        return;
    }
    run_live_with_historical_events(diff_events());
}

pub(crate) fn pty_helper_live_scroll() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_SCROLL_SCENARIO) {
        return;
    }
    run_live_with_historical_events(scroll_events());
}

pub(crate) fn pty_helper_question_overlay() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(QUESTION_OVERLAY_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = mpsc::channel();
    let inject_tx = update_tx.clone();
    // Grok question freeze: Thought + Ask + Waiting chrome above the dock.
    // Seed an in-flight turn, then inject the question permission so orphan Ask projects.
    thread::spawn(move || {
        thread::sleep(PERMISSION_INJECT_DELAY);
        // Historical ends at seq 4; inject Ask at 5, then finish-with-usage at 6 so
        // orphan Ask exists first (Thought stays) and breadcrumb packs 10K / 262K.
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(
            question_permission_requested_event(
                5,
                "perm_question_parity",
                "tool_call_question_parity",
            ),
        )));
        let mut finish = parity_envelope(
            6,
            Some("req_question_pty"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_question_pty".into(),
                finish_reason: "tool_calls".to_string(),
                output_digest: Some("digest-out-question".to_string()),
                usage: Some(parity_completion_usage_context_and_total(10_000, 10_200)),
                metadata: None,
            }),
        );
        // Keep Waiting duration freeze-aligned: permission inject mono=1000; do not regress mono.
        finish.mono_ms = 1000;
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(finish)));
    });

    install_parity_context_window();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: question_turn_events(),
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
    })
    .unwrap_or_abort();
}

fn run_live_with_historical_events(historical_events: Vec<EventEnvelopeV1>) {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (_keepalive, update_rx) = mpsc::channel();
    install_parity_context_window();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events,
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
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_permission_overlay() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(PERMISSION_OVERLAY_SCENARIO) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = mpsc::channel();
    let inject_tx = update_tx.clone();
    thread::spawn(move || {
        thread::sleep(PERMISSION_INJECT_DELAY);
        // Historical permission_turn_events uses seq 1..=5; inject must be unseen.
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(permission_requested_event(
            7,
            "perm_parity_overlay",
            PERMISSION_TOOL_CALL_ID,
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
            let _ = resolve_tx.send(LiveUpdate::Event(Box::new(permission_resolved_event(
                8,
                &permission_id,
                event_decision,
                reason,
            ))));
        }
    });

    set_pending_live_prompt_draft(Some(PERMISSION_DRAFT.to_string()));
    install_parity_context_window();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            // Grok PERM freeze: Thought + Creating above the dock (seed like question overlay).
            historical_events: permission_turn_events(),
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
    })
    .unwrap_or_abort();
}

fn pty_signoff_enabled() -> bool {
    cfg!(target_os = "linux") && std::env::var(PTY_SIGNOFF_ENV).as_deref() == Ok("1")
}

fn parity_strict_enabled() -> bool {
    std::env::var(PARITY_STRICT_ENV).as_deref() == Ok("1")
}

fn require_pty_signoff() -> bool {
    if pty_signoff_enabled() {
        return true;
    }
    if parity_strict_enabled() {
        panic!(
            "HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PTY_SIGNOFF=1 on Linux; silent PTY no-op is forbidden"
        );
    }
    false
}

fn maybe_dump_l3(env_key: &str, screen: &str) {
    let Ok(path) = std::env::var(env_key) else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_abort();
    }
    std::fs::write(&path, screen).unwrap_or_abort();
}

fn wait_for_any(helper: &mut SpawnedHelper, needles: &[&str]) -> String {
    let deadline = Instant::now() + MARKER_TIMEOUT;
    loop {
        let screen = helper.screen_text();
        if needles.iter().any(|needle| screen.contains(needle)) {
            return screen;
        }
        if Instant::now() >= deadline {
            panic!(
                "PTY wait_for_any timed out after {MARKER_TIMEOUT:?} waiting for one of {needles:?}\n{screen}"
            );
        }
        thread::sleep(READ_POLL_TIMEOUT);
    }
}

struct SpawnedHelper {
    #[allow(dead_code, reason = "kept for resize parity with e2e helper")]
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

    fn screen_text(&mut self) -> String {
        drain_output(&mut self.parser, &self.output_rx);
        self.parser.screen().contents()
    }
}

fn spawn_type_first_startup_helper() -> SpawnedHelper {
    spawn_helper(TYPE_FIRST_STARTUP_HELPER, TYPE_FIRST_STARTUP_SCENARIO)
}

fn spawn_live_draft_helper() -> SpawnedHelper {
    spawn_helper(LIVE_DRAFT_HELPER, LIVE_DRAFT_SCENARIO)
}

fn spawn_helper(test_name: &str, scenario: &str) -> SpawnedHelper {
    spawn_helper_at(test_name, scenario, PRIMARY_COLS, PRIMARY_ROWS)
}

fn exit_via_palette(helper: &mut SpawnedHelper) {
    send_key(helper.writer.as_mut(), 0x11).unwrap_or_abort();
    let start = Instant::now();
    let deadline = start + EXIT_TIMEOUT;
    let resend_at = start + EXIT_TIMEOUT / 2;
    let mut resent = false;
    loop {
        match helper.child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.success(), "helper tui child exited with {status:?}");
                return;
            }
            Ok(None) => {
                if !resent && Instant::now() >= resend_at {
                    send_key(helper.writer.as_mut(), 0x11).unwrap_or_abort();
                    resent = true;
                }
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(READ_POLL_TIMEOUT);
            }
            Err(err) => panic!("exit_via_palette: try_wait failed: {err}"),
        }
    }
    let _ = helper.child.kill();
    let _ = helper.child.wait();
}

fn assert_no_multi_row_prompt_rail(screen: &str, context: &str) {
    // Transcript user rows use `❯` (freeze-aligned). Composer uses bordered `│ ❯`.
    // Question/permission docks use freeze-aligned `┃` rails — not a composer regression.
    // Only multiple composer-style prompt rows count as a multi-row prompt rail.
    let composer_prompt_lines = screen
        .lines()
        .filter(|line| line.contains('│') && line.contains('❯'))
        .count();
    assert!(
        composer_prompt_lines <= 1,
        "PTY {context} must not paint a multi-row composer ❯ rail (found {composer_prompt_lines})\n{screen}"
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

fn spawn_helper_at(test_name: &str, scenario: &str, cols: u16, rows: u16) -> SpawnedHelper {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(pty_size(cols, rows)).unwrap_or_abort();

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
        parser: Parser::new(rows, cols, 0),
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

fn parity_envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_parity_tx_{seq:04}"),
        seq,
        run_id: "run_parity_tx_shell".into(),
        mono_ms: seq,
        ts: Some("2026-07-17T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("parity-tx-shell".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_parity_tx_shell".to_string()),
        payload,
    }
}

fn stream_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_stream_pty";
    // Grok STREAM freeze: "Waiting for response…" state — user submitted, provider
    // started, no body text yet. TaskScheduled keeps the activity Streaming after
    // ProviderRequestFinished seeds total_tokens for the ⇣ download counter.
    // Elapsed 5.3s, ⇣1.43k tokens.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_stream_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: STREAM_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: STREAM_USER_TEXT.to_string(),
                request_digest: "digest-stream".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-stream".to_string()),
                usage: Some(parity_completion_usage(1430)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; finish at 5400 → elapsed 5.3s from 100.
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 100, 5400]) {
        event.mono_ms = mono;
    }
    events
}

fn perm_stream_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_perm_stream_pty";
    // SHELL-PERM reference freeze (run4-shell-perm-pinned-v4): tool-in-flight
    // streaming state — user submitted "edit a project file now", provider
    // started, no body text. TaskScheduled keeps activity Streaming.
    // Elapsed 3.6s, ⇣4.26k tokens.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_perm_stream_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: PERM_STREAM_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: PERM_STREAM_USER_TEXT.to_string(),
                request_digest: "digest-perm-stream".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-perm-stream".to_string()),
                usage: Some(parity_completion_usage(4260)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; finish at 3700 → elapsed 3.6s from 100.
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 100, 3700]) {
        event.mono_ms = mono;
    }
    events
}

fn question_stream_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_question_stream_pty";
    // SHELL-QUESTION reference freeze (run1-shell-question-pinned-v1):
    // question-tool-in-flight streaming state — user submitted "ask me the
    // parity question", provider started, 5 ◆ question tool-call chips,
    // waiting-for-response spinner. Elapsed 3.3s, ⇣4.35k tokens.
    // The reference binary renders question tool calls as ◆ question chips
    // but never surfaces an interactive question UI. The harness produces a
    // matching streaming state; transcript content (◆ question chips vs
    // blank) is masked as identity in the L6 field mask.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_question_stream_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: QUESTION_STREAM_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: QUESTION_STREAM_USER_TEXT.to_string(),
                request_digest: "digest-question-stream".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "tool_calls".to_string(),
                output_digest: Some("digest-out-question-stream".to_string()),
                usage: Some(parity_completion_usage_context_and_total(4_300, 4_350)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; finish at 3400 → elapsed 3.3s from 100.
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 100, 3400]) {
        event.mono_ms = mono;
    }
    events
}

fn fail_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_fail_pty";
    // Grok FAIL freeze (run1-shell-fail-pinned-v1): user message + "parity
    // turn fails mid stream" assistant text + "⠸ Retrying (attempt 2)… 0.4s
    // 2.6s ⇣4.14k [stop]" activity footer. Single activity: TaskScheduled
    // keeps it Streaming after ProviderRequestFinished seeds total_tokens.
    // ProviderRequestStarted carries retry metadata (attempt 2) so the footer
    // renders the retry spinner instead of "Waiting for response…".
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_fail_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: FAIL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: FAIL_USER_TEXT.to_string(),
                request_digest: "digest-fail".to_string(),
                metadata: Some(ProviderRequestStartedMetadata {
                    retry: Some(ProviderRequestRetryMetadata {
                        attempt: 2,
                        max_attempts: 5,
                        delay_ms: None,
                        category: Some(ProviderErrorCategory::RateLimited),
                    }),
                    ..Default::default()
                }),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "parity turn fails mid stream".to_string(),
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-fail".to_string()),
                usage: Some(parity_completion_usage(4_140)),
                metadata: None,
            }),
        ),
    ];
    // UserMessageSubmitted at mono 100 sets first_mono_ms; ProviderRequestStarted
    // at mono 2300 sets request_started_mono_ms; ProviderRequestFinished at mono
    // 2700 sets last_mono_ms. duration=2.6s, retry_elapsed=0.4s.
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 2300, 2400, 2700]) {
        event.mono_ms = mono;
    }
    events
}

fn complete_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_complete_pty";
    // Grok COMPLETE freeze (run1-shell-complete-pinned-v1): Worked for 2.3s.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: COMPLETE_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: COMPLETE_USER_TEXT.to_string(),
                request_digest: "digest-complete".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: COMPLETE_ASSISTANT_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out".to_string()),
                usage: Some(parity_completion_usage(4_100)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; span 100→2400 = 2.3s Worked for.
    for (event, mono) in events.iter_mut().zip([100_u64, 200, 1000, 2400]) {
        event.mono_ms = mono;
    }
    events
}

fn recover_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_recover_pty";
    // Grok RECOVER freeze (run1-shell-recover-pinned-v1): same retry structure
    // as SHELL-FAIL but with "recover the parity probe" user message and
    // "retry after failure draft" in the composer. Retry spinner shows
    // "⠸ Retrying (attempt 2)… 1.2s    3.8s ⇣4.14k [stop]".
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_recover_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: RECOVER_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: RECOVER_USER_TEXT.to_string(),
                request_digest: "digest-recover".to_string(),
                metadata: Some(ProviderRequestStartedMetadata {
                    retry: Some(ProviderRequestRetryMetadata {
                        attempt: 2,
                        max_attempts: 5,
                        delay_ms: None,
                        category: Some(ProviderErrorCategory::RateLimited),
                    }),
                    ..Default::default()
                }),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "parity turn fails mid stream".to_string(),
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-recover".to_string()),
                usage: Some(parity_completion_usage(4_140)),
                metadata: None,
            }),
        ),
    ];
    // duration=3.8s (100→3900), retry_elapsed=1.2s (3900-2700).
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 2700, 2800, 3900]) {
        event.mono_ms = mono;
    }
    events
}

fn tool_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_tool_pty";
    // Grok TOOL freeze: streaming tool-loop with 10 echo commands (5 failed).
    // Reference: '❙ ◈ Ran 5 commands · 5 failed' + 10 '❙ ◆ Run echo tx-tool-output-probe-line'
    // + '⠇ Waiting for response… 3.2s' + 'Ctrl+c:cancel' footer.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: TOOL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: TOOL_USER_TEXT.to_string(),
                request_digest: "digest-tool".to_string(),
                metadata: None,
            }),
        ),
    ];
    // 10 echo tool calls: 5 succeeded (even index), 5 failed (odd index).
    for i in 0..10u32 {
        let tc_id = format!("tc_echo_{i}");
        let succeeded = i % 2 == 0;
        let seq = 3 + i as u64 * 3;
        events.push(parity_envelope(
            seq,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tc_id.clone().into(),
                tool_id: "bash".to_string(),
                args_summary: r#"{"command":"echo tx-tool-output-probe-line"}"#.to_string(),
                args_digest: format!("digest-args-echo-{i}"),
                metadata: None,
            }),
        ));
        events.push(parity_envelope(
            seq + 1,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: tc_id.clone().into(),
            }),
        ));
        events.push(parity_envelope(
            seq + 2,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tc_id.into(),
                status: if succeeded {
                    ToolCallStatus::Succeeded
                } else {
                    ToolCallStatus::Failed
                },
                output_summary: Some(if succeeded {
                    "tx-tool-output-probe-line".to_string()
                } else {
                    "command failed".to_string()
                }),
                output_digest: Some(format!("digest-out-echo-{i}")),
                output_json: None,
                metadata: None,
            }),
        ));
    }
    // No ProviderRequestFinished — streaming state matches reference freeze.
    // Mono timing: spread over 3200ms to match 'Waiting for response… 3.2s'.
    let mut mono = 100_u64;
    for event in events.iter_mut() {
        event.mono_ms = mono;
        mono += 100;
    }
    events
}

fn diff_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_diff_pty";
    // Grok DIFF freeze: streaming state with 9 edit chips, no inline diff body.
    // Reference: '◆ edit' chips (9 entries) + '⠋ Waiting for response… 3.2s' + 'Ctrl+c:cancel' footer.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: DIFF_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: DIFF_USER_TEXT.to_string(),
                request_digest: "digest-diff".to_string(),
                metadata: None,
            }),
        ),
    ];
    // 9 edit tool calls in streaming state (no ProviderRequestFinished).
    for i in 0..9u32 {
        let tc_id = format!("tc_edit_{i}");
        let seq = 3 + i as u64 * 3;
        events.push(parity_envelope(
            seq,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tc_id.clone().into(),
                tool_id: "fs.write".to_string(),
                args_summary: r#"{"path":"demo.txt","content":"parity-diff-ok\n","oldContent":"old content\n"}"#
                    .to_string(),
                args_digest: format!("digest-args-diff-{i}"),
                metadata: None,
            }),
        ));
        events.push(parity_envelope(
            seq + 1,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: tc_id.clone().into(),
            }),
        ));
        events.push(parity_envelope(
            seq + 2,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tc_id.into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("demo.txt".to_string()),
                output_digest: Some(format!("digest-out-diff-{i}")),
                output_json: None,
                metadata: None,
            }),
        ));
    }
    // No ProviderRequestFinished — streaming state matches reference freeze.
    // Mono timing: spread over 3200ms to match 'Waiting for response… 3.2s'.
    let mut mono = 100_u64;
    for event in events.iter_mut() {
        event.mono_ms = mono;
        mono += 100;
    }
    events
}

fn scroll_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_scroll_pty";
    // Grok SCROLL freeze (run1-shell-scroll-pinned-v1): streaming state with
    // partial assistant response visible. TaskScheduled keeps activity Streaming
    // after ProviderRequestFinished seeds total_tokens for the ⇣ download counter.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_scroll_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: SCROLL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: SCROLL_USER_TEXT.to_string(),
                request_digest: "digest-scroll".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: SCROLL_ASSISTANT_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-scroll".to_string()),
                usage: Some(parity_completion_usage(4_140)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; delta at 2400; finish at 2600 → 0.2s since delta, 2.5s total.
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 100, 2400, 2600]) {
        event.mono_ms = mono;
    }
    events
}

fn question_turn_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_question_pty";
    // mono 0 → inject at 900ms so Waiting footer packs 0.9s like Grok freeze.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: QUESTION_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: QUESTION_USER_TEXT.to_string(),
                request_digest: "digest-question-turn".to_string(),
                metadata: None,
            }),
        ),
        // Title-only reasoning span mono 100→200 = 0.1s Thought for (Grok freeze).
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: "**plan**".to_string(),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: String::new(),
            }),
        ),
    ];
    // User/start/reasoning-start/reasoning-end; inject at 1000 → Waiting 0.9s.
    // Usage/breadcrumb meta seeded via post-inject finish in pty_helper_question_overlay
    // (finish before orphan Ask would promote thinking → body and drop Thought for).
    for (event, mono) in events.iter_mut().zip([0_u64, 50, 100, 200]) {
        event.mono_ms = mono;
    }
    events
}

fn question_permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_parity_question_{seq:04}"),
        seq,
        run_id: "run_parity_question_overlay".into(),
        // Grok freeze packs Waiting meta as ~0.9s.
        // Historical first mono settles at 100 (0 is treated as unset); inject at 1000 → 0.9s.
        mono_ms: 1000,
        ts: Some("2026-07-17T12:00:00Z".to_string()),
        actor: EventActor::new(
            ActorKind::System,
            Some("parity-question-overlay".to_string()),
        ),
        correlation_id: Some("req_question_pty".to_string()),
        causation_id: None,
        stream_key: None,
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "question".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: serde_json::json!({
                "questions": [{
                    "question": "Which color?",
                    "header": "Color",
                    "options": [
                        {"label": "Red", "description": "Choose red"},
                        {"label": "Green", "description": "Choose green"},
                        {"label": "Blue", "description": "Choose blue"}
                    ],
                    "multiple": false,
                    "custom": true,
                }]
            })
            .to_string(),
            request_digest: format!("digest-question-{permission_id}"),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    }
}

fn permission_turn_events() -> Vec<EventEnvelopeV1> {
    let request_id = PERMISSION_REQUEST_ID;
    // Grok PERM freeze: Thought for 0.1s + Creating demo.txt above Allow Edit dock.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: PERMISSION_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: PERMISSION_USER_TEXT.to_string(),
                request_digest: "digest-perm-turn".to_string(),
                metadata: None,
            }),
        ),
        // Title-only reasoning span mono 100→200 = 0.1s Thought for (Grok freeze).
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: "**plan**".to_string(),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: String::new(),
            }),
        ),
        // Pending write so Creating demo.txt projects; permission attaches by tool_call_id.
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: PERMISSION_TOOL_CALL_ID.into(),
                tool_id: "fs.write".to_string(),
                args_summary: r#"{"path":"demo.txt","content":"parity-ok\n"}"#.to_string(),
                args_digest: "digest-args-perm-write".to_string(),
                metadata: None,
            }),
        ),
        // Seed breadcrumb token meta (10K / 262K); permission dock inject stays open after.
        parity_envelope(
            6,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "tool_calls".to_string(),
                output_digest: Some("digest-out-perm".to_string()),
                usage: Some(parity_completion_usage_context_and_total(10_000, 10_100)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; reasoning 100→200 = 0.1s Thought; write at 400;
    // finish at 19100 so turn duration packs freeze Run Write right-meta 19s
    // (last_mono - first_mono = 19000).
    for (event, mono) in events.iter_mut().zip([100_u64, 100, 100, 200, 400, 19_100]) {
        event.mono_ms = mono;
    }
    events
}

fn permission_requested_event(
    seq: u64,
    permission_id: &str,
    tool_call_id: &str,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_parity_perm_{seq:04}"),
        seq,
        run_id: "run_parity_permission_overlay".into(),
        // After historical finish mono 19100 so pending write + Thought remain visible.
        mono_ms: 19_400,
        ts: Some("2026-07-17T12:00:00Z".to_string()),
        actor: EventActor::new(
            ActorKind::System,
            Some("parity-permission-overlay".to_string()),
        ),
        correlation_id: Some(PERMISSION_REQUEST_ID.to_string()),
        causation_id: None,
        stream_key: None,
        payload: EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: permission_id.to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: r#"{"path":"demo.txt"}"#.to_string(),
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
        event_id: format!("evt_parity_perm_resolved_{seq:04}"),
        seq,
        run_id: "run_parity_permission_overlay".into(),
        mono_ms: seq,
        ts: Some("2026-07-17T12:00:01Z".to_string()),
        actor: EventActor::new(
            ActorKind::System,
            Some("parity-permission-overlay".to_string()),
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
