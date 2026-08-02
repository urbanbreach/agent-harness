// allow: SIZE_OK — reference-parity PTY owner matrix + helper spawn surface
//! PTY interaction owners for reference-parity first-slice and overlay rows.
//!
//! Separate from `pty_e2e_impl` so both modules stay reviewable. Spawns this
//! test binary as a helper child with scenario env vars (same pattern as e2e).

use harness_core::event::{
    ActorKind, CompactionAppliedEvent, EventActor, EventEnvelopeV1, EventV1,
    PermissionRequestedEvent, ProviderReasoningDeltaEvent, ProviderRequestFinishedEvent,
    ProviderRequestRetryMetadata, ProviderRequestStartedEvent, ProviderRequestStartedMetadata,
    ProviderStreamDeltaEvent, RunStartedEvent, TaskScheduleState, TaskScheduledEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
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
const STARTUP_CLIPBOARD_WARNING: &str =
    "System clipboard may be inaccessible, copy will use OSC 52.";
const IDLE_SHELL_SCENARIO: &str = "idle_shell";
const LIVE_DRAFT_SCENARIO: &str = "live_draft";
const LIVE_STREAM_SCENARIO: &str = "live_stream";
const LIVE_FAIL_SCENARIO: &str = "live_fail";
const LIVE_COMPLETE_SCENARIO: &str = "live_complete";
const LIVE_MERMAID_SCENARIO: &str = "live_mermaid";
const LIVE_CANCEL_SCENARIO: &str = "live_cancel";
const LIVE_RECOVER_SCENARIO: &str = "live_recover";
const LIVE_TOOL_SCENARIO: &str = "live_tool";
const LIVE_RUNNING_TOOL_SCENARIO: &str = "live_running_tool";
const LIVE_MIXED_TRANSCRIPT_SCENARIO: &str = "live_mixed_transcript";
const LIVE_MIXED_TRANSCRIPT_DONE_SCENARIO: &str = "live_mixed_transcript_done";
const LIVE_THINKING_ANIMATION_SCENARIO: &str = "live_thinking_animation";
const LIVE_DIFF_SCENARIO: &str = "live_diff";
const LIVE_SCROLL_SCENARIO: &str = "live_scroll";
const QUESTION_OVERLAY_SCENARIO: &str = "question_overlay";
const QUESTION_OVERLAY_OVERFLOW_SCENARIO: &str = "question_overlay_overflow";
const QUESTION_OVERLAY_WRAPPED_SCENARIO: &str = "question_overlay_wrapped";
const PERMISSION_OVERLAY_SCENARIO: &str = "permission_overlay";
/// Empty-draft variant: same permission overlay but without seeding a draft, so
/// the capture verifies "Allow Edit" chrome renders with an empty composer.
const PERMISSION_OVERLAY_EMPTY_DRAFT_SCENARIO: &str = "permission_overlay_empty_draft";
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
const TYPE_FIRST_STARTUP_HELPER: &str = "reference_parity_pty_helper_type_first_startup";
const IDLE_SHELL_HELPER: &str = "pty_helper_idle_shell";
const LIVE_DRAFT_HELPER: &str = "pty_helper_live_draft";
const LIVE_STREAM_HELPER: &str = "pty_helper_live_stream";
const LIVE_FAIL_HELPER: &str = "pty_helper_live_fail";
const LIVE_COMPLETE_HELPER: &str = "pty_helper_live_complete";
const LIVE_MERMAID_HELPER: &str = "pty_helper_live_mermaid";
const LIVE_CANCEL_HELPER: &str = "pty_helper_live_cancel";
const LIVE_RECOVER_HELPER: &str = "pty_helper_live_recover";
const LIVE_TOOL_HELPER: &str = "pty_helper_live_tool";
const LIVE_MIXED_TRANSCRIPT_HELPER: &str = "pty_helper_live_mixed_transcript";
const LIVE_MIXED_TRANSCRIPT_DONE_HELPER: &str = "pty_helper_live_mixed_transcript_done";
const LIVE_DIFF_HELPER: &str = "pty_helper_live_diff";
const LIVE_SCROLL_HELPER: &str = "pty_helper_live_scroll";
const QUESTION_OVERLAY_HELPER: &str = "pty_helper_question_overlay";
const PERMISSION_OVERLAY_HELPER: &str = "reference_parity_pty_helper_permission_overlay";
const LIVE_PERM_STREAM_HELPER: &str = "pty_helper_live_perm_stream";
const LIVE_QUESTION_STREAM_HELPER: &str = "pty_helper_live_question_stream";
const STREAM_USER_TEXT: &str = "stream parity probe";
const PERM_STREAM_USER_TEXT: &str = "edit a project file now";
const QUESTION_STREAM_USER_TEXT: &str = "ask me the parity question";
const FAIL_USER_TEXT: &str = "fail the parity probe";
const COMPLETE_USER_TEXT: &str = "complete the parity probe";
const COMPLETE_ASSISTANT_TEXT: &str = "parity turn complete stream final response rendered cleanly under the shell composer parity turn complete stream final response rendered cleanly under the shell composer parity turn complete stream final response rendered cleanly under the shell composer";
const MERMAID_USER_TEXT: &str = "render a Mermaid flowchart";
const MERMAID_ASSISTANT_TEXT: &str =
    "多言語: 你好，世界 · こんにちは\n```mermaid\ngraph TD\n  A[Start] --> B[Build]\n  B --> C[Done]\n```";
// Reference cancellation state (run1-shell-cancel-pinned-v1): empty transcript + draft in composer.
const CANCEL_USER_TEXT: &str = "cancel the parity probe";
// Reference recovery state (run1-shell-recover-pinned-v1): same fail state + draft in composer.
const RECOVER_USER_TEXT: &str = "recover the parity probe";
const RECOVER_DRAFT: &str = "retry after failure draft";
const TOOL_USER_TEXT: &str = "run the echo probe";
const TOOL_PATH_TEXT: &str = "echo tx-tool-output-probe-line";
const DIFF_USER_TEXT: &str = "edit the probe file";

// Freeze breadcrumb token meta uses context window 262K and turn usage like 12K / 10K / 1.5K.
const PARITY_CONTEXT_WINDOW_TOKENS: u32 = 262_144;
const TX_CONTEXT_WINDOW_TOKENS: u32 = 8_192;

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

fn parity_model_metadata_for_scenario(scenario: Option<&str>) -> (&'static str, u32) {
    match scenario {
        Some(LIVE_TOOL_SCENARIO) => ("Mock TX-Tool Model", TX_CONTEXT_WINDOW_TOKENS),
        Some(LIVE_MIXED_TRANSCRIPT_SCENARIO | LIVE_MIXED_TRANSCRIPT_DONE_SCENARIO) => {
            ("Mock Mixed Model", TX_CONTEXT_WINDOW_TOKENS)
        }
        Some(LIVE_DIFF_SCENARIO) => ("Mock TX-Diff Model", TX_CONTEXT_WINDOW_TOKENS),
        Some(IDLE_SHELL_SCENARIO) => (
            "Umans Kimi K2.7 (CLIProxy) (xhigh) · always-approve",
            PARITY_CONTEXT_WINDOW_TOKENS,
        ),
        Some(LIVE_STREAM_SCENARIO) => ("Mock Stream Model", PARITY_CONTEXT_WINDOW_TOKENS),
        Some(LIVE_PERM_STREAM_SCENARIO) => ("Mock Perm Model", PARITY_CONTEXT_WINDOW_TOKENS),
        Some(LIVE_FAIL_SCENARIO | LIVE_RECOVER_SCENARIO) => {
            ("Mock Error Model", PARITY_CONTEXT_WINDOW_TOKENS)
        }
        Some(LIVE_COMPLETE_SCENARIO | LIVE_CANCEL_SCENARIO) => {
            ("Mock Turn Model", PARITY_CONTEXT_WINDOW_TOKENS)
        }
        _ => ("Parity Test Model (Mock)", PARITY_CONTEXT_WINDOW_TOKENS),
    }
}

fn install_parity_context_window() {
    let (model_label, context_window_tokens) =
        parity_model_metadata_for_scenario(std::env::var(HELPER_SCENARIO_ENV).ok().as_deref());
    let option = ModelOption {
        profile: "parity".to_string(),
        provider: "mock".to_string(),
        provider_display_label: None,
        provider_backend_label: None,
        model: "model-tx".to_string(),
        model_display_label: Some(model_label.to_string()),
        variant: None,
        variant_display_label: None,
        display_label: Some(model_label.to_string()),
        token_window_label: None,
        context_window_tokens: Some(context_window_tokens),
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
        text_verbosity: None,
        thinking: None,
        recommended_for: None,
    };
    set_pending_replay_launch_metadata(Some(LaunchMetadata::from_model_option(&option)));
}
// Reference scroll state (run1-shell-scroll-pinned-v1): streaming state with partial response.
const SCROLL_USER_TEXT: &str = "scroll the parity probe";
const SCROLL_ASSISTANT_TEXT: &str =
    "parity turn complete stream final response rendered cleanly under the shell composer parity turn complete stream final response rendered";
const QUESTION_USER_TEXT: &str = "You MUST use the AskUserQuestion tool (or equivalent question tool) to ask me exactly one multiple-choice question: Which color? Options: Red, Green, Blue. Do not answer yourself. Do not use any other tools.";
const READY_MARKER: &str = "❯";
const DRAFT_TEXT: &str = "parity draft";
const PERMISSION_DRAFT: &str = "keep draft under permission";
// Reference permission-state packing: Thought + Creating demo.txt above edit permission dock.
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
    assert!(
        !screen.contains("No provider connected"),
        "reference startup capture must use a connected provider fixture\n{screen}"
    );
    assert!(
        screen.contains("Clipboard may be unreachable.") && screen.contains("/terminal-setup"),
        "PTY startup must expose the canonical clipboard capability warning\n{screen}"
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
    let screen = wait_for_any(&mut helper, &["Resume session", "Replay session"]);
    assert!(
        screen.contains("Resume session") || screen.contains("Replay session"),
        "OVL-SESSION PTY: session picker overlay title required\n{screen}"
    );
    assert_no_sidebar_copy(&screen, "session picker");
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
        screen.contains("◆ edit"),
        "SHELL-PERM PTY: in-flight edit tool hierarchy must project\n{screen}"
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
        !screen.contains("Retrying"),
        "SHELL-STREAM PTY: initial provider attempts must not render retry chrome\n{screen}"
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
    helper.wait_for("Worked for 2.3s");
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
        screen.contains("Enter:queue") && !screen.contains("Ctrl+Enter:send now"),
        "SHELL-RECOVER PTY: active retry draft must expose only the reference-visible queue action\n{screen}"
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
    helper.wait_for("parity turn complete stream final response rendered cleanly");
    // Reference scroll state: PageUp during streaming to scroll away from follow.
    send_bytes(helper.writer.as_mut(), b"\x1b[5~").unwrap_or_abort();
    thread::sleep(READ_POLL_TIMEOUT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(SCROLL_USER_TEXT),
        "SHELL-SCROLL PTY: user message required\n{screen}"
    );
    assert!(
        screen.contains("parity turn complete stream final response rendered cleanly"),
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
    helper.wait_for("Ran 6 commands");
    helper.wait_for(TOOL_PATH_TEXT);
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    assert!(
        screen.contains(TOOL_PATH_TEXT),
        "TX-TOOL PTY: command detail rows remain visible beneath the group summary\n{screen}"
    );
    assert!(
        screen.contains("Waiting for response") && !screen.contains("Responding"),
        "TX-TOOL PTY: terminal tool loop must remain in waiting-for-response state\n{screen}"
    );
    assert!(
        screen.contains("Ran 6 commands · 6 failed") && screen.contains("◆ Run echo"),
        "TX-TOOL PTY: completed command groups keep Grok-style member rows visible\n{screen}"
    );
    assert_eq!(
        screen.matches("◆ Run echo").count(),
        12,
        "TX-TOOL PTY: command groups must render every projected member row\n{screen}"
    );
    assert_eq!(
        screen.matches('┃').count(),
        13,
        "TX-TOOL PTY: grouped command summary and all members share the continuous Grok rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-TOOL");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_diff_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_DIFF_HELPER, LIVE_DIFF_SCENARIO);
    helper.wait_for("◆ edit");
    helper.wait_for("Waiting for response");
    let screen = helper.screen_text();
    assert!(
        screen.matches("◆ edit").count() == 9 && !screen.contains("demo.txt"),
        "TX-DIFF PTY: compact edit projection must match the nine pathless reference rows\n{screen}"
    );
    assert!(
        screen.matches("◆ edit").count() == 9 && screen.contains("Waiting for response"),
        "TX-DIFF PTY: nine edit chips and the waiting-for-response state must remain visible\n{screen}"
    );
    assert!(
        !screen.contains('❙') && screen.matches('┃').count() == 1,
        "TX-DIFF PTY: the final compact edit row carries the selected Grok rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-DIFF");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_mixed_transcript_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_MIXED_TRANSCRIPT_HELPER,
        LIVE_MIXED_TRANSCRIPT_SCENARIO,
        120,
        40,
    );
    helper.wait_for("Created, inspected, and verified the file successfully.");
    let screen = helper.screen_text();
    maybe_dump_l3("HARNESS_MIXED_TRANSCRIPT_L3_DUMP", &screen);
    let lines = screen.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.contains("Inspecting the temporary test file"))
        .unwrap_or_abort();
    let end = lines
        .iter()
        .position(|line| line.contains("Created, inspected, and verified the file successfully."))
        .unwrap_or_abort();
    let visible = &lines[start..=end];
    let maximum_blank_run = visible
        .split(|line| !line.trim().is_empty())
        .map(<[&str]>::len)
        .max()
        .unwrap_or(0);
    assert!(
        maximum_blank_run <= 1,
        "TX-MIXED PTY: interleaved response and tool rows must remain dense\n{screen}"
    );
    let ordered = [
        "Inspecting the temporary test file",
        "I'll inspect the temporary test file.",
        "Verifying the file once more",
        "The first tool completed; now I'll verify it once more.",
        "Created, inspected, and verified the file successfully.",
    ];
    let mut cursor = 0;
    for needle in ordered {
        let position = screen[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("TX-MIXED PTY: missing {needle:?}\n{screen}"));
        cursor += position + needle.len();
    }
    assert_eq!(
        screen.matches("◆ Read mixed.txt").count(),
        2,
        "TX-MIXED PTY: both read calls must remain visible\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-MIXED");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_mixed_transcript_done_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        LIVE_MIXED_TRANSCRIPT_DONE_HELPER,
        LIVE_MIXED_TRANSCRIPT_DONE_SCENARIO,
        120,
        40,
    );
    helper.wait_for("Worked for");
    let screen = helper.screen_text();
    maybe_dump_l3("HARNESS_MIXED_TRANSCRIPT_DONE_L3_DUMP", &screen);
    let lines = screen.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.contains("Thought for"))
        .unwrap_or_abort();
    let end = lines
        .iter()
        .position(|line| line.contains("Worked for"))
        .unwrap_or_abort();
    let maximum_blank_run = lines[start..=end]
        .split(|line| !line.trim().is_empty())
        .map(<[&str]>::len)
        .max()
        .unwrap_or(0);
    assert!(
        maximum_blank_run <= 1,
        "TX-MIXED-DONE PTY: completed mixed transcript must stay densely packed\n{screen}"
    );
    assert!(
        !screen.contains("Inspecting the temporary test file")
            && !screen.contains("Verifying the file once more"),
        "TX-MIXED-DONE PTY: completed reasoning bodies must auto-collapse\n{screen}"
    );
    assert!(
        screen.contains("The first tool completed; now I'll verify it once more."),
        "TX-MIXED-DONE PTY: prose between tool calls must remain visible\n{screen}"
    );
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_tool_and_diff_fixtures_match_pinned_transcript_geometry() {
    let tool_events = tool_events();
    let diff_events = diff_events();

    assert_eq!(
        parity_model_metadata_for_scenario(Some(LIVE_TOOL_SCENARIO)),
        ("Mock TX-Tool Model", TX_CONTEXT_WINDOW_TOKENS)
    );
    assert_eq!(
        parity_model_metadata_for_scenario(Some(LIVE_DIFF_SCENARIO)),
        ("Mock TX-Diff Model", TX_CONTEXT_WINDOW_TOKENS)
    );

    let tool_prompt = tool_events.iter().find_map(|event| match &event.payload {
        EventV1::UserMessageSubmitted(data) => Some(data.text.as_str()),
        _ => None,
    });
    assert_eq!(tool_prompt, Some("run the echo probe"));
    assert_eq!(
        tool_events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallRequested(_)))
            .count(),
        12
    );
    assert_eq!(
        tool_events
            .iter()
            .filter(|event| {
                matches!(
                    event.payload,
                    EventV1::ToolCallFinished(ToolCallFinishedEvent {
                        status: ToolCallStatus::Failed,
                        ..
                    })
                )
            })
            .count(),
        6
    );
    assert_eq!(
        tool_events.iter().find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data) =>
                data.usage.as_ref().map(|usage| usage.total_tokens),
            _ => None,
        }),
        Some(4_650)
    );

    let diff_prompt = diff_events.iter().find_map(|event| match &event.payload {
        EventV1::UserMessageSubmitted(data) => Some(data.text.as_str()),
        _ => None,
    });
    assert_eq!(diff_prompt, Some("edit the probe file"));
    assert_eq!(
        diff_events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallRequested(_)))
            .count(),
        9
    );
    assert_eq!(
        diff_events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
            .count(),
        9
    );
    assert_eq!(
        diff_events.iter().find_map(|event| match &event.payload {
            EventV1::ProviderRequestFinished(data) =>
                data.usage.as_ref().map(|usage| usage.total_tokens),
            _ => None,
        }),
        Some(4_500)
    );
    assert!(
        diff_events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallStarted(data) if data.tool_call_id.as_str() == "tc_edit_8"
            )
        }) && diff_events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data) if data.tool_call_id.as_str() == "tc_edit_8"
            )
        })
    );
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
    assert!(
        !screen.contains("Starting session"),
        "{label} PTY: settled idle shell must not retain startup seed\n{screen}"
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

    let (keepalive, update_rx) = mpsc::channel();
    keepalive
        .send(LiveUpdate::AuthBackendResult {
            success: true,
            message: "provider fixture connected".to_string(),
        })
        .unwrap_or_abort();
    keepalive
        .send(LiveUpdate::Status(STARTUP_CLIPBOARD_WARNING.to_string()))
        .unwrap_or_abort();
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
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

#[allow(
    deprecated,
    reason = "legacy compaction event seeds idle context usage"
)]
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
        model_display_label: Some(
            "Umans Kimi K2.7 (CLIProxy) (xhigh) · always-approve".to_string(),
        ),
        variant: None,
        variant_display_label: None,
        display_label: Some("Umans Kimi K2.7 (CLIProxy) (xhigh) · always-approve".to_string()),
        token_window_label: None,
        context_window_tokens: Some(PARITY_CONTEXT_WINDOW_TOKENS),
        max_input_tokens: None,
        max_output_tokens: None,
        description: None,
        profile_description: None,
        reasoning_effort: None,
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
            last_updated_at: Some("2026-07-28T12:00:00Z".to_string()),
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
            historical_events: vec![
                parity_envelope(
                    0,
                    None,
                    EventV1::RunStarted(RunStartedEvent {
                        run_name: "idle-shell".into(),
                        workspace_root: "/home/urbanbreach/Projects/agent-harness".to_string(),
                    }),
                ),
                parity_envelope(
                    1,
                    None,
                    EventV1::CompactionApplied(CompactionAppliedEvent {
                        checkpoint_id: "checkpoint_idle_shell".to_string(),
                        agent_id: "agent_idle_shell".to_string(),
                        through_seq: 0,
                        through_request_id: None,
                        tokens_before_estimate: Some(1_700),
                        tokens_after_estimate: Some(1_700),
                        summary_tokens_estimate: None,
                        compacted_turns: None,
                        preserved_turns: None,
                        reduction_tokens_estimate: None,
                        reduction_percent_estimate: None,
                        estimate_source: None,
                    }),
                ),
            ],
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
        skip_alternate_screen: false,
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
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_live_stream() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_STREAM_SCENARIO) {
        return;
    }
    run_live_with_live_events(stream_events());
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

pub(crate) fn pty_helper_live_mermaid() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_MERMAID_SCENARIO) {
        return;
    }
    run_live_with_historical_events(mermaid_events());
}

pub(crate) fn pty_helper_live_cancel() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_CANCEL_SCENARIO) {
        return;
    }
    // Reference cancellation state (run1-shell-cancel-pinned-v1): empty transcript + draft
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
    run_live_with_live_events(recover_events());
}

pub(crate) fn pty_helper_live_tool() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_TOOL_SCENARIO) {
        return;
    }
    run_live_with_historical_events(tool_events());
}

pub(crate) fn pty_helper_live_mixed_transcript() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_MIXED_TRANSCRIPT_SCENARIO) {
        return;
    }
    run_live_with_historical_events(mixed_transcript_events());
}

pub(crate) fn pty_helper_live_mixed_transcript_done() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_MIXED_TRANSCRIPT_DONE_SCENARIO) {
        return;
    }
    run_live_with_historical_events(mixed_transcript_done_events());
}

pub(crate) fn pty_helper_live_thinking_animation() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_THINKING_ANIMATION_SCENARIO) {
        return;
    }
    run_live_with_timed_reasoning();
}

pub(crate) fn pty_helper_live_running_tool() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_RUNNING_TOOL_SCENARIO) {
        return;
    }
    run_live_with_historical_events(running_tool_events());
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
    run_question_overlay_helper(QUESTION_OVERLAY_SCENARIO, false, false);
}

pub(crate) fn pty_helper_question_overlay_overflow() {
    run_question_overlay_helper(QUESTION_OVERLAY_OVERFLOW_SCENARIO, true, false);
}

pub(crate) fn pty_helper_question_overlay_wrapped() {
    run_question_overlay_helper(QUESTION_OVERLAY_WRAPPED_SCENARIO, true, true);
}

fn run_question_overlay_helper(scenario: &str, overflow: bool, wrapped: bool) {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(scenario) {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = mpsc::channel();
    let inject_tx = update_tx.clone();
    // Reference question state: Thought + Ask + Waiting chrome above the dock.
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
                overflow,
                wrapped,
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
        skip_alternate_screen: false,
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
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

fn run_live_with_live_events(events: Vec<EventEnvelopeV1>) {
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = mpsc::channel();
    let inject_tx = update_tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(25));
        for event in events {
            let _ = inject_tx.send(LiveUpdate::Event(Box::new(event)));
        }
    });
    install_parity_context_window();
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
    drop(update_tx);
}

fn run_live_with_timed_reasoning() {
    let request_id = "req_thinking_animation_pty";
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = mpsc::channel();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(1_800));
        let mut events = thinking_animation_events(request_id).into_iter();
        let user = events.next().unwrap_or_abort();
        let _ = update_tx.send(LiveUpdate::Event(Box::new(user)));
        thread::sleep(Duration::from_millis(50));
        let started = events.next().unwrap_or_abort();
        let _ = update_tx.send(LiveUpdate::Event(Box::new(started)));
        thread::sleep(Duration::from_millis(50));
        for (index, event) in events.enumerate() {
            let _ = update_tx.send(LiveUpdate::Event(Box::new(event)));
            let delay = match index {
                0 => 600,
                _ => 300,
            };
            thread::sleep(Duration::from_millis(delay));
        }
        thread::park();
    });
    install_parity_context_window();
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
            // Reference permission state: Thought + Creating above the dock (seed like question overlay).
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
        skip_alternate_screen: false,
    })
    .unwrap_or_abort();
}

pub(crate) fn pty_helper_permission_overlay_empty_draft() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(PERMISSION_OVERLAY_EMPTY_DRAFT_SCENARIO)
    {
        return;
    }

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let (update_tx, update_rx) = mpsc::channel();
    let inject_tx = update_tx.clone();
    thread::spawn(move || {
        thread::sleep(PERMISSION_INJECT_DELAY);
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(permission_requested_event(
            7,
            "perm_parity_empty_draft",
            PERMISSION_TOOL_CALL_ID,
        ))));
    });

    install_parity_context_window();
    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: permission_turn_events(),
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
    // Reference streaming state: "Waiting for response…" state — user submitted, provider
    // started, no body text yet. TaskScheduled keeps the activity Streaming after
    // ProviderRequestFinished seeds context/turn usage. The PTY helper injects these
    // through the live-update channel so both timers advance from the runtime clock.
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
                metadata: Some(ProviderRequestStartedMetadata {
                    retry: Some(ProviderRequestRetryMetadata {
                        attempt: 0,
                        max_attempts: 3,
                        delay_ms: None,
                        category: None,
                    }),
                    ..ProviderRequestStartedMetadata::default()
                }),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-stream".to_string()),
                usage: Some(parity_completion_usage_context_and_total(1_400, 1_430)),
                metadata: None,
            }),
        ),
    ];
    for event in &mut events {
        event.mono_ms = 100;
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
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_perm_stream_edit".into(),
                tool_id: "edit".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-perm-stream-edit".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_perm_stream_edit".into(),
            }),
        ),
        parity_envelope(
            6,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_perm_stream_edit".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: None,
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            7,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_perm_stream_edit_2".into(),
                tool_id: "edit".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-perm-stream-edit-2".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            8,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_perm_stream_edit_2".into(),
            }),
        ),
        parity_envelope(
            9,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_perm_stream_edit_2".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: None,
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            10,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_perm_stream_edit_3".into(),
                tool_id: "edit".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-perm-stream-edit-3".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            11,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_perm_stream_edit_3".into(),
            }),
        ),
        parity_envelope(
            12,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_perm_stream_edit_3".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: None,
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            13,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_perm_stream_edit_4".into(),
                tool_id: "edit".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "digest-perm-stream-edit-4".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            14,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_perm_stream_edit_4".into(),
            }),
        ),
        parity_envelope(
            15,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_perm_stream_edit_4".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: None,
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            16,
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
    for event in &mut events {
        event.mono_ms = 100;
    }
    if let Some(finished) = events.last_mut() {
        finished.mono_ms = 3700;
    }
    events
}

fn question_stream_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_question_stream_pty";
    // SHELL-QUESTION reference freeze (run1-shell-question-pinned-v1):
    // question-tool-in-flight streaming state — user submitted "ask me the
    // parity question", provider started, 5 ◆ question tool-call chips,
    // waiting-for-response spinner. Elapsed 3.3s, ⇣4.35k tokens.
    // The reference binary renders the five failed question tool attempts as
    // compact red ◆ question rows before returning to the live wait state.
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
    ];
    for index in 0..5_u64 {
        let tool_call_id = format!("tc_question_stream_{index}");
        let seq = 4 + index * 3;
        events.push(parity_envelope(
            seq,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tool_call_id.clone().into(),
                tool_id: "question".to_string(),
                args_summary: "{}".to_string(),
                args_digest: format!("digest-question-stream-{index}"),
                metadata: None,
            }),
        ));
        events.push(parity_envelope(
            seq + 1,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: tool_call_id.clone().into(),
            }),
        ));
        events.push(parity_envelope(
            seq + 2,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tool_call_id.into(),
                status: ToolCallStatus::Failed,
                output_summary: None,
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));
    }
    events.push(parity_envelope(
        19,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "tool_calls".to_string(),
            output_digest: Some("digest-out-question-stream".to_string()),
            usage: Some(parity_completion_usage_context_and_total(4_300, 4_350)),
            metadata: None,
        }),
    ));
    // first mono non-zero; finish at 3400 → elapsed 3.3s from 100.
    for event in &mut events {
        event.mono_ms = 100;
    }
    if let Some(finished) = events.last_mut() {
        finished.mono_ms = 3_400;
    }
    events
}

fn fail_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_fail_pty";
    let retry_request_id = "req_fail_retry_pty";
    // Reference failure state (run1-shell-fail-pinned-v1): user message + "parity
    // turn fails mid stream" assistant text + "⠸ Retrying (attempt 2)… 0.4s
    // 2.6s ⇣4.14k [stop]" activity footer. Single activity: TaskScheduled
    // keeps it Streaming after ProviderRequestFinished seeds total_tokens.
    // ProviderRequestStarted carries retry metadata (attempt 2) so the footer
    // renders the retry spinner instead of "Waiting for response…".
    let mut events = vec![
        parity_envelope(
            1,
            Some(retry_request_id),
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
                metadata: None,
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
                usage: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            6,
            Some(retry_request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: retry_request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: FAIL_USER_TEXT.to_string(),
                request_digest: "digest-fail-retry".to_string(),
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
            7,
            Some(retry_request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: retry_request_id.into(),
                delta: "parity turn fails mid stream".to_string(),
            }),
        ),
        parity_envelope(
            8,
            Some(retry_request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: retry_request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-fail-retry".to_string()),
                usage: Some(parity_completion_usage(4_140)),
                metadata: None,
            }),
        ),
    ];
    // UserMessageSubmitted at mono 100 sets first_mono_ms; ProviderRequestStarted
    // at mono 2300 sets request_started_mono_ms; ProviderRequestFinished at mono
    // 2700 sets last_mono_ms. duration=2.6s, retry_elapsed=0.4s.
    for (event, mono) in events
        .iter_mut()
        .zip([100_u64, 100, 100, 100, 2300, 2300, 2400, 2700])
    {
        event.mono_ms = mono;
    }
    events
}

fn complete_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_complete_pty";
    // Reference completed state (run1-shell-complete-pinned-v1): Worked for 2.3s.
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

fn mermaid_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_mermaid_pty";
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: MERMAID_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: MERMAID_USER_TEXT.to_string(),
                request_digest: "digest-mermaid".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: MERMAID_ASSISTANT_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out-mermaid".to_string()),
                usage: Some(parity_completion_usage(4_100)),
                metadata: None,
            }),
        ),
    ];
    for (event, mono) in events.iter_mut().zip([100_u64, 200, 1_000, 2_400]) {
        event.mono_ms = mono;
    }
    events
}

fn recover_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_recover_pty";
    let retry_request_id = "req_recover_retry_pty";
    // Reference recovery state (run1-shell-recover-pinned-v1): same retry structure
    // as SHELL-FAIL but with "recover the parity probe" user message and
    // "retry after failure draft" in the composer. Retry spinner shows
    // "⠸ Retrying (attempt 2)… 1.2s    3.8s ⇣4.14k [stop]".
    let mut events = vec![
        parity_envelope(
            1,
            Some(retry_request_id),
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
                metadata: None,
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
                usage: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            6,
            Some(retry_request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: retry_request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: RECOVER_USER_TEXT.to_string(),
                request_digest: "digest-recover-retry".to_string(),
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
            7,
            Some(retry_request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: retry_request_id.into(),
                delta: "parity turn fails mid stream".to_string(),
            }),
        ),
        parity_envelope(
            8,
            Some(retry_request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: retry_request_id.into(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-out-recover-retry".to_string()),
                usage: Some(parity_completion_usage(4_140)),
                metadata: None,
            }),
        ),
    ];
    // duration=3.8s (100→3900), retry_elapsed=1.2s (3900-2700).
    for (event, mono) in events
        .iter_mut()
        .zip([100_u64, 100, 100, 100, 2600, 2700, 2800, 3900])
    {
        event.mono_ms = mono;
    }
    events
}

fn tool_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_tool_pty";
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_tool_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: TOOL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
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
    for i in 0..12u32 {
        let tc_id = format!("tc_echo_{i}");
        let succeeded = i < 6;
        let seq = 4 + u64::from(i) * 3;
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
    // ProviderRequestFinished seeds the token counter; TaskScheduled(Started)
    // keeps the activity Streaming so "Waiting for response" renders.
    events.push(parity_envelope(
        40,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "tool_calls".to_string(),
            output_digest: Some("digest-out-tool".to_string()),
            usage: Some(parity_completion_usage(4_650)),
            metadata: None,
        }),
    ));
    // Mono timing: spread over 3200ms to match 'Waiting for response… 3.2s'.
    let mut mono = 100_u64;
    for event in events.iter_mut() {
        event.mono_ms = mono;
        event.ts = Some("2026-07-17T20:15:00Z".to_string());
        mono += 85;
    }
    events
}

fn running_tool_events() -> Vec<EventEnvelopeV1> {
    let mut events = tool_events();
    events.truncate(5);
    events
}

fn mixed_transcript_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_mixed_transcript_pty";
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_mixed_transcript_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "go".to_string(),
            }),
        ),
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: "go".to_string(),
                request_digest: "digest-mixed".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: "Inspecting the temporary test file".to_string(),
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "I'll inspect the temporary test file.".to_string(),
            }),
        ),
        parity_envelope(
            6,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_mixed_read_one".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"mixed.txt"}"#.to_string(),
                args_digest: "digest-mixed-read-one".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            7,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_mixed_read_one".into(),
            }),
        ),
        parity_envelope(
            8,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_mixed_read_one".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("1 file".to_string()),
                output_digest: Some("digest-mixed-read-one-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            9,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: "Verifying the file once more".to_string(),
            }),
        ),
        parity_envelope(
            10,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "The first tool completed; now I'll verify it once more.".to_string(),
            }),
        ),
        parity_envelope(
            11,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_mixed_read_two".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"mixed.txt"}"#.to_string(),
                args_digest: "digest-mixed-read-two".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            12,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_mixed_read_two".into(),
            }),
        ),
        parity_envelope(
            13,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_mixed_read_two".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("1 file".to_string()),
                output_digest: Some("digest-mixed-read-two-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            14,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Created, inspected, and verified the file successfully.".to_string(),
            }),
        ),
    ];
    for (event, mono) in events.iter_mut().zip((0_u64..).map(|index| index * 5)) {
        event.mono_ms = mono;
    }
    events
}

fn thinking_animation_events(request_id: &str) -> Vec<EventEnvelopeV1> {
    let deltas = [
        "THOUGHTFOLDSENTINEL",
        " weighing",
        " which",
        "",
        " fixture",
        " file",
    ];
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: "go".to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "test-model".to_string(),
                prompt_summary: "go".to_string(),
                request_digest: "digest-thinking-animation".to_string(),
                metadata: None,
            }),
        ),
    ];
    let reasoning_mono_ms = [0, 400, 700, 1_000, 1_200, 1_550];
    events.extend(deltas.into_iter().enumerate().map(|(index, delta)| {
        let mut event = parity_envelope(
            u64::try_from(index).unwrap_or_default() + 3,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: delta.to_string(),
            }),
        );
        event.mono_ms = reasoning_mono_ms[index];
        event
    }));
    events
}

fn mixed_transcript_done_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_mixed_transcript_pty";
    let mut events = mixed_transcript_events();
    events.retain(|event| !matches!(event.payload, EventV1::TaskScheduled(_)));
    let mut finished = parity_envelope(
        15,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-mixed-done".to_string()),
            usage: Some(parity_completion_usage(120)),
            metadata: None,
        }),
    );
    finished.mono_ms = 100;
    events.push(finished);
    events
}

fn diff_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_diff_pty";
    // Reference diff state: streaming state with 9 edit chips, no inline diff body.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_diff_parity".to_string().into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:mock:model-tx".to_string()),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: DIFF_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            3,
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
    for i in 0..9u32 {
        let tc_id = format!("tc_edit_{i}");
        let seq = 4 + u64::from(i) * 3;
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
    events.push(parity_envelope(
        31,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "tool_calls".to_string(),
            output_digest: Some("digest-out-diff".to_string()),
            usage: Some(parity_completion_usage(4_500)),
            metadata: None,
        }),
    ));
    events.push(parity_envelope(
        32,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: DIFF_USER_TEXT.to_string(),
            request_digest: "digest-diff-waiting".to_string(),
            metadata: None,
        }),
    ));
    let mut mono = 100_u64;
    for event in events.iter_mut() {
        event.mono_ms = mono;
        event.ts = Some("2026-07-17T20:17:00Z".to_string());
        mono += 100;
    }
    events
}

fn scroll_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_scroll_pty";
    // Reference scroll state (run1-shell-scroll-pinned-v1): streaming state with
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
    // mono 0 → inject at 900ms so Waiting footer packs 0.9s in the waiting state.
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
        // Title-only reasoning span mono 100→200 = 0.1s Thought for (waiting state).
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
    overflow: bool,
    wrapped: bool,
) -> EventEnvelopeV1 {
    let options = if overflow {
        (1..=24)
            .map(|index| {
                serde_json::json!({
                    "label": format!("地域 {index}"),
                    "description": if wrapped {
                        format!("Choose deployment region {index} because this deliberately long explanation must wrap across several visual rows")
                    } else {
                        format!("Choose deployment region {index}")
                    }
                })
            })
            .collect::<Vec<_>>()
    } else {
        vec![
            serde_json::json!({"label": "Red", "description": "Choose red"}),
            serde_json::json!({"label": "Green", "description": "Choose green"}),
            serde_json::json!({"label": "Blue", "description": "Choose blue"}),
        ]
    };
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_parity_question_{seq:04}"),
        seq,
        run_id: "run_parity_question_overlay".into(),
        // Waiting state packs Waiting meta as ~0.9s.
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
                    "question": if overflow { "Choose a deployment region 日本語" } else { "Which color?" },
                    "header": if overflow { "Region" } else { "Color" },
                    "options": options,
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
    // Waiting state: Thought for 0.1s + Creating demo.txt above Allow Edit dock.
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
        // Title-only reasoning span mono 100→200 = 0.1s Thought for (waiting state).
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
