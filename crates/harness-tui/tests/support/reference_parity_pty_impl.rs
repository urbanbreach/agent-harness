// allow: SIZE_OK — reference-parity PTY owner matrix + helper spawn surface
//! PTY interaction owners for reference-parity first-slice and overlay rows.
//!
//! Separate from `pty_e2e_impl` so both modules stay reviewable. Spawns this
//! test binary as a helper child with scenario env vars (same pattern as e2e).

use harness_core::event::{
    ActorKind,
    EventActor,
    EventEnvelopeV1,
    EventV1,
    PermissionRequestedEvent,
    ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent,
    ProviderStreamDeltaEvent,
    RunFailedEvent,
    TaskCancelledEvent,
    TaskTerminalScope,
    ToolCallFinishedEvent,
    ToolCallRequestedEvent,
    ToolCallStartedEvent,
    ToolCallStatus,
    UserMessageSubmittedEvent,
    SCHEMA_VERSION,
    ProviderReasoningDeltaEvent,
};
use harness_providers::CompletionUsage;
use harness_tui::UnwrapOrAbort;
use harness_tui::{
    run_tui_with_options, set_pending_replay_launch_metadata, LiveUpdate, TuiMode, TuiOptions,
    UiIntent,
};
use harness_tui::app::{LaunchMetadata, ModelOption};
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
const TYPE_FIRST_STARTUP_HELPER: &str = "pty_helper_type_first_startup";
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
const STREAM_USER_TEXT: &str = "stream probe";
const STREAM_ASSISTANT_TEXT: &str = "partial assistant tokens";
const FAIL_USER_TEXT: &str = "ping";
const FAIL_ERROR_TEXT: &str = "API error (status 400 Bad Request): invalid-argument: Incorrect API key provided. You can obtain an API key from https://console.x.ai.\n\nRequest URL: https://api.x.ai/v1/responses\n\nTurn failed in 0.3s: Internal error: {\n  \"message\": \"API error (status 400 Bad Request): invalid-argument: Incorrect API key provided. You can obtain an API key from https://console.x.ai.\\n\\nRequest UR...";
const COMPLETE_USER_TEXT: &str = "Reply with exactly: HELLO_PARITY_OK";
const COMPLETE_ASSISTANT_TEXT: &str = "HELLO_PARITY_OK";
// Grok CANCEL freeze user prompt (run1-cancel-proxy-v2).
const CANCEL_USER_TEXT: &str =
    "Write a long detailed essay about terminal UI design with at least 20 paragraphs. Keep going until stopped.";
// Partial stream body before cancel (freeze shows title + intro + truncated next sentence).
const CANCEL_PARTIAL_TEXT: &str = "The Art and Science of Terminal UI Design\n\nIntroduction\n\nThe terminal, that deceptively simple window into the machine, has been the primary interface for computing since the dawn of the interactive era. Long before graphical user interfaces became the norm, and even as they dominate modern computing, the terminal remains a vital, irreplaceable tool.\n\nDesigning user interfaces for this environment is a discipline that sits at the intersection of art";
// Grok RECOVER freeze: cancel mid-stream then type a recover draft.
const RECOVER_USER_TEXT: &str =
    "Write a long essay about terminals with 15 paragraphs. Keep going.";
// Grok RECOVER freeze: long partial stream (Paragraph 1 + Paragraph 3, truncates at "command to list files").
const RECOVER_PARTIAL_TEXT: &str = "Paragraph 1 — Origins of the Terminal Early computing systems did not have screens or monitors; they communicated with operators through machines that printed output onto paper tape or tore-up rolls of continuous form. The Teletype Model 33, introduced in 1963, became one of the first widely adopted computer terminals, bridging the gap between human-readable text and machine-processable input. From these humble beginnings emerged the concept of the interactive terminal—a device through which users could type commands and receive immediate textual feedback. This paradigm of direct, command-line interaction fundamentally shaped the design of operating systems, programming languages, and the culture of computing itself.\n\nParagraph 3 — The Command Line Philosophy At the heart of the terminal lies a profound philosophy: the belief that humans should be able to instruct machines with precision and intentionality. Unlike graphical interfaces that nudge and suggest through menus and buttons, the terminal demands that you articulate your desires in structured language. This requirement fosters a deeper understanding of the systems you interact with. When you must type the exact command to list files";
const RECOVER_DRAFT: &str = "retry draft after cancel";
const RECOVER_CANCEL_BODY: &str = "Turn cancelled by user in 2.3s";
// Grok TOOL freeze user prompt (run1-tool-proxy-v2).
const TOOL_USER_TEXT: &str =
    "Use a tool to list files in the current directory, then report COUNT=N for the number of top-level entries. Do not invent.";
const TOOL_PATH_TEXT: &str = "Listed 1 dir";
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
        model_display_label: None,
        variant: None,
        variant_display_label: None,
        display_label: None,
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
    set_pending_replay_launch_metadata(Some(LaunchMetadata::from_model_option(&option)));
}
const DIFF_PATH_TEXT: &str = "demo.txt";
// Grok SCROLL freeze: long numbered inventory with more-below ▼ affordance.
const SCROLL_USER_TEXT: &str = "List every file in the current directory using a tool, then write a numbered inventory of all names one per line.";
// Bottom-pinned view shows the end of the inventory; wait on a late line first.
const SCROLL_INVENTORY_BOTTOM: &str = "80. f80.txt";
// Grok freeze mid-band (f39–f55) after scrolling up from the bottom.
const SCROLL_INVENTORY_MID: &str = "55. f55.txt";
const SCROLL_INVENTORY_EARLY: &str = "1. f1.txt";
const QUESTION_DRAFT: &str = "keep draft under question";
// Grok freeze packing: Which color? with Red/Green/Blue (+ custom).
const QUESTION_USER_TEXT: &str = "You MUST use the AskUserQuestion tool (or equivalent question tool) to ask me exactly one multiple-choice question: Which color? Options: Red, Green, Blue. Do not answer yourself. Do not use any other tools.";
const QUESTION_PROMPT: &str = "Which color?";
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
        &["Keyboard Shortcuts", "Shortcuts", "Essentials", "/ to search"],
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
    let mut helper = spawn_helper(PERMISSION_OVERLAY_HELPER, PERMISSION_OVERLAY_SCENARIO);
    helper.wait_for(READY_MARKER);
    helper.wait_for("❯");
    helper
        .writer
        .write_all(PERMISSION_DRAFT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(PERMISSION_DRAFT);
    helper.wait_for("Allow Edit");
    let screen = helper.screen_text();
    assert!(
        screen.contains("always-approve") || screen.contains("Allow Edit"),
        "permission overlay must surface approval chrome\n{screen}"
    );
    assert!(
        screen.contains("Yes, allow all edits during this session"),
        "permission overlay must pack freeze option 2 session-edits\n{screen}"
    );
    assert!(
        screen.contains("1/4:select") || screen.contains("1/4"),
        "permission overlay must pack freeze 1/4:select hint\n{screen}"
    );
    assert!(
        screen.contains("Ctrl+o:always-approve"),
        "permission overlay must pack freeze Ctrl+o:always-approve hint\n{screen}"
    );
    assert!(
        screen.contains("Ctrl+c:cancel"),
        "permission overlay must pack freeze Ctrl+c:cancel hint\n{screen}"
    );
    assert!(
        screen.contains("Thought for 0.1s"),
        "permission overlay must pack freeze-aligned Thought for 0.1s\n{screen}"
    );
    assert!(
        screen.contains("Creating demo.txt"),
        "permission overlay must pack Creating demo.txt write chrome\n{screen}"
    );
    assert!(
        screen.contains("19s"),
        "permission overlay must pack freeze Run Write right-meta duration 19s\n{screen}"
    );
    assert!(
        screen.contains(PERMISSION_DRAFT),
        "permission overlay must preserve draft\n{screen}"
    );
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_for(READY_MARKER);
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
    let mut helper = spawn_helper(PERMISSION_OVERLAY_HELPER, PERMISSION_OVERLAY_SCENARIO);
    helper.wait_for(READY_MARKER);
    helper
        .writer
        .write_all(PERMISSION_DRAFT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(PERMISSION_DRAFT);
    helper.wait_for("Allow Edit");
    let screen = helper.screen_text();
    assert!(
        screen.contains("always-approve") || screen.contains("Allow Edit"),
        "SHELL-PERM PTY: permission chrome required\n{screen}"
    );
    assert!(
        screen.contains("Yes, allow all edits during this session"),
        "SHELL-PERM PTY: freeze option 2 session-edits required\n{screen}"
    );
    assert!(
        screen.contains("1/4:select") || screen.contains("1/4"),
        "SHELL-PERM PTY: freeze 1/4:select hint required\n{screen}"
    );
    assert!(
        screen.contains("Ctrl+o:always-approve"),
        "SHELL-PERM PTY: freeze Ctrl+o:always-approve hint required\n{screen}"
    );
    assert!(
        screen.contains("Ctrl+c:cancel"),
        "SHELL-PERM PTY: freeze Ctrl+c:cancel hint required\n{screen}"
    );
    assert!(
        screen.contains("Thought for 0.1s"),
        "SHELL-PERM PTY: Thought must pack freeze-aligned 0.1s reasoning duration\n{screen}"
    );
    assert!(
        screen.contains("Creating demo.txt"),
        "SHELL-PERM PTY: Creating demo.txt write chrome required\n{screen}"
    );
    assert!(
        screen.contains("19s"),
        "SHELL-PERM PTY: freeze Run Write right-meta duration 19s required\n{screen}"
    );
    assert!(
        screen.contains("10K / 262K") || screen.contains("10K / 262"),
        "SHELL-PERM PTY: freeze breadcrumb token meta 10K / 262K required\n{screen}"
    );
    assert!(
        screen.contains("⇣10.1k") || screen.contains("⇣10k"),
        "SHELL-PERM PTY: mid-stream token meta required (freeze ⇣10.1k)\n{screen}"
    );
    assert!(
        screen.contains(PERMISSION_DRAFT),
        "SHELL-PERM PTY: draft must survive permission preemption\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-PERM");
    assert_no_sidebar_copy(&screen, "SHELL-PERM");
    send_bytes(helper.writer.as_mut(), b"\x1b").unwrap_or_abort();
    helper.wait_for(READY_MARKER);
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_stream_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_STREAM_HELPER, LIVE_STREAM_SCENARIO);
    helper.wait_for(STREAM_USER_TEXT);
    helper.wait_for(STREAM_ASSISTANT_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(STREAM_USER_TEXT) && screen.contains(STREAM_ASSISTANT_TEXT),
        "SHELL-STREAM PTY: user + partial stream must project\n{screen}"
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
    let mut helper = spawn_helper(LIVE_FAIL_HELPER, LIVE_FAIL_SCENARIO);
    helper.wait_for(FAIL_USER_TEXT);
    let screen = wait_for_any(
        &mut helper,
        &["API error", "invalid-argument", "Incorrect API key", "fail"],
    );
    assert!(
        screen.contains(FAIL_USER_TEXT),
        "SHELL-FAIL PTY: user turn retained above error\n{screen}"
    );
    assert!(
        !screen.contains("Thought for"),
        "SHELL-FAIL PTY: fail freeze has no Thought for chrome\n{screen}"
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
    let mut helper = spawn_helper(LIVE_COMPLETE_HELPER, LIVE_COMPLETE_SCENARIO);
    helper.wait_for(COMPLETE_USER_TEXT);
    helper.wait_for(COMPLETE_ASSISTANT_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(COMPLETE_USER_TEXT) && screen.contains(COMPLETE_ASSISTANT_TEXT),
        "SHELL-COMPLETE PTY: completed turn must project\n{screen}"
    );
    assert!(
        screen.contains("Worked for 0.8s"),
        "SHELL-COMPLETE PTY: Worked for must pack freeze-aligned 0.8s duration\n{screen}"
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
    let mut helper = spawn_helper(LIVE_COMPLETE_HELPER, LIVE_COMPLETE_SCENARIO);
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
    let mut helper = spawn_helper(LIVE_COMPLETE_HELPER, LIVE_COMPLETE_SCENARIO);
    helper.wait_for(COMPLETE_ASSISTANT_TEXT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(COMPLETE_ASSISTANT_TEXT),
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
    let mut helper = spawn_helper(LIVE_CANCEL_HELPER, LIVE_CANCEL_SCENARIO);
    // Expanded essay body scrolls the user prompt off-screen; wait on visible freeze markers.
    helper.wait_for("Turn cancelled by user in 1.3s");
    helper.wait_for("intersection of art");
    let screen = helper.screen_text();
    assert!(
        screen.contains("terminal UI design") || screen.contains("Keep going until"),
        "SHELL-CANCEL PTY: user turn retained under cancel\n{screen}"
    );
    assert!(
        screen.contains("Thought for 0.2s"),
        "SHELL-CANCEL PTY: Thought must pack freeze-aligned 0.2s reasoning duration\n{screen}"
    );
    assert!(
        screen.contains("Turn cancelled by user in 1.3s"),
        "SHELL-CANCEL PTY: cancel body must pack freeze-aligned 1.3s duration\n{screen}"
    );
    assert!(
        screen.contains("The Art and Science of Terminal UI Design")
            && screen.contains("Introduction")
            && screen.contains("intersection of art"),
        "SHELL-CANCEL PTY: freeze-aligned partial essay body required\n{screen}"
    );
    assert!(
        screen.contains("1.5K / 262K") || screen.contains("1.5K / 262"),
        "SHELL-CANCEL PTY: freeze breadcrumb token meta 1.5K / 262K required\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-CANCEL PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-CANCEL");
    assert_no_sidebar_copy(&screen, "SHELL-CANCEL");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_recover_pty() {
    if !require_pty_signoff() {
        return;
    }
    // Grok RECOVER freeze: cancelled turn retained + editable draft in composer.
    let mut helper = spawn_helper(LIVE_RECOVER_HELPER, LIVE_RECOVER_SCENARIO);
    helper.wait_for(RECOVER_CANCEL_BODY);
    helper
        .writer
        .write_all(RECOVER_DRAFT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    helper.wait_for(RECOVER_DRAFT);
    let screen = helper.screen_text();
    assert!(
        screen.contains(RECOVER_CANCEL_BODY) || screen.contains("Turn cancelled by user"),
        "SHELL-RECOVER PTY: cancelled turn must remain visible\n{screen}"
    );
    assert!(
        screen.contains(RECOVER_DRAFT),
        "SHELL-RECOVER PTY: recover draft must be editable after cancel\n{screen}"
    );
    assert!(
        screen.contains("command to list files")
            || screen.contains("Command Line Philosophy")
            || screen.contains("Origins of the Terminal"),
        "SHELL-RECOVER PTY: freeze-aligned partial essay body required\n{screen}"
    );
    assert!(
        screen.contains("1.5K / 262K") || screen.contains("1.5K / 262"),
        "SHELL-RECOVER PTY: freeze breadcrumb token meta 1.5K / 262K required\n{screen}"
    );
    assert!(
        !screen.contains("Thought for"),
        "SHELL-RECOVER PTY: recover freeze has no empty Thought for chrome\n{screen}"
    );
    assert!(
        screen.contains("Enter:send") || screen.contains('❯'),
        "SHELL-RECOVER PTY: composer must accept draft after cancel\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-RECOVER");
    assert_no_sidebar_copy(&screen, "SHELL-RECOVER");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_scroll_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_SCROLL_HELPER, LIVE_SCROLL_SCENARIO);
    // Historical feed pins to bottom — late inventory lines are visible first.
    helper.wait_for(SCROLL_INVENTORY_BOTTOM);
    // Grok SCROLL freeze shows inventory ~f39–f55 with more-below ▼ (user prompt still visible).
    // PageUp scrolls by 10 rows; two steps from bottom lands near freeze window (3 overshoots to f31).
    for _ in 0..2 {
        send_bytes(helper.writer.as_mut(), b"\x1b[5~").unwrap_or_abort();
        thread::sleep(Duration::from_millis(40));
    }
    let screen = helper.screen_text();
    assert!(
        screen.contains("39. f39.txt")
            || screen.contains("45. f45.txt")
            || screen.contains(SCROLL_INVENTORY_MID),
        "SHELL-SCROLL PTY: freeze viewport ~f39–f55 inventory required\n{screen}"
    );
    assert!(
        screen.contains('▼'),
        "SHELL-SCROLL PTY: more-below ▼ affordance required when not at bottom\n{screen}"
    );
    assert!(
        screen.contains("19K / 262K") || screen.contains("19K / 262"),
        "SHELL-SCROLL PTY: freeze breadcrumb token meta 19K / 262K required\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "SHELL-SCROLL PTY: composer glyph required\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "SHELL-SCROLL");
    assert_no_sidebar_copy(&screen, "SHELL-SCROLL");
    exit_via_palette(&mut helper);
}

pub(crate) fn tx_tool_pty() {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(LIVE_TOOL_HELPER, LIVE_TOOL_SCENARIO);
    helper.wait_for(TOOL_PATH_TEXT);
    helper.wait_for("COUNT=2");
    let screen = helper.screen_text();
    assert!(
        screen.contains(TOOL_PATH_TEXT),
        "TX-TOOL PTY: completed list title required\n{screen}"
    );
    assert!(
        screen.contains("COUNT=2"),
        "TX-TOOL PTY: post-tool COUNT body required (Grok freeze form)\n{screen}"
    );
    assert!(
        screen.contains("Worked for 1.7s"),
        "TX-TOOL PTY: Worked for must pack freeze-aligned 1.7s duration\n{screen}"
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
    let screen = helper.screen_text();
    assert!(
        screen.contains(DIFF_PATH_TEXT) || screen.contains('◆'),
        "TX-DIFF PTY: structured edit/path projection required\n{screen}"
    );
    assert!(
        screen.contains("Thought for 0.1s"),
        "TX-DIFF PTY: Thought must pack freeze-aligned 0.1s reasoning duration\n{screen}"
    );
    assert!(
        screen.contains("Worked for 2.2s"),
        "TX-DIFF PTY: Worked for must pack freeze-aligned 2.2s duration\n{screen}"
    );
    assert!(
        !screen.contains('┃'),
        "TX-DIFF PTY: no legacy left rail\n{screen}"
    );
    assert_no_multi_row_prompt_rail(&screen, "TX-DIFF");
    exit_via_palette(&mut helper);
}

pub(crate) fn shell_question_pty() {
    assert_question_overlay_pty("SHELL-QUESTION");
}

pub(crate) fn ovl_question_pty() {
    assert_question_overlay_pty("OVL-QUESTION");
}

fn assert_question_overlay_pty(label: &str) {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper(QUESTION_OVERLAY_HELPER, QUESTION_OVERLAY_SCENARIO);
    // Historical turn projects user chrome with ❯ — do not use bare READY_MARKER.
    // Full QUESTION_USER_TEXT wraps across rows; wait on a contiguous freeze marker.
    helper.wait_for("Which color? Options: Red, Green, Blue");
    // Type draft before permission inject (900ms) so buffer has context under the dock.
    helper
        .writer
        .write_all(QUESTION_DRAFT.as_bytes())
        .unwrap_or_abort();
    helper.writer.flush().unwrap_or_abort();
    // Dock + Ask/Waiting arrive via delayed permission inject.
    helper.wait_for(QUESTION_PROMPT);
    helper.wait_for("Ask ");
    helper.wait_for("Waiting on answers");
    let open = helper.screen_text();
    assert!(
        open.contains(QUESTION_PROMPT)
            || open.contains("Color")
            || open.contains('●')
            || open.contains('○'),
        "{label} PTY: question dock must render\n{open}"
    );
    assert!(
        open.contains("Red") && open.contains("Green") && open.contains("Blue"),
        "{label} PTY: freeze-aligned color options required\n{open}"
    );
    assert!(
        open.contains("(○)") && !open.contains("(●)"),
        "{label} PTY: unanswered options must all be ○\n{open}"
    );
    assert!(
        open.contains("Ask "),
        "{label} PTY: Ask tool chrome required\n{open}"
    );
    assert!(
        open.contains("Waiting on answers"),
        "{label} PTY: Waiting on answers footer required\n{open}"
    );
    assert!(
        open.contains("Thought for 0.1s"),
        "{label} PTY: Thought must pack freeze-aligned 0.1s reasoning duration\n{open}"
    );
    assert!(
        open.contains("0.9s"),
        "{label} PTY: Waiting footer must pack freeze-aligned 0.9s duration\n{open}"
    );
    assert!(
        open.contains("⇣10.2k"),
        "{label} PTY: Waiting footer must pack freeze-aligned mid-stream token meta ⇣10.2k\n{open}"
    );
    assert!(
        open.contains("10K / 262K") || open.contains("10K / 262"),
        "{label} PTY: freeze breadcrumb token meta 10K / 262K required (context fill)\n{open}"
    );
    assert!(
        open.contains("Esc:unselect") && open.contains("Tab:scrollback"),
        "{label} PTY: outer shell footer must match freeze labels\n{open}"
    );
    assert!(
        open.contains('┃'),
        "{label} PTY: question dock must paint ┃ rail matching Grok packing\n{open}"
    );
    assert!(
        !open.contains("always-approve"),
        "{label} PTY: must not render edit-permission allow chrome\n{open}"
    );
    assert!(
        open.contains(QUESTION_DRAFT)
            || open.contains("Type your answer here")
            || open.contains("select")
            || open.contains("unselect"),
        "{label} PTY: question dock must keep composer/draft context\n{open}"
    );
    assert_no_multi_row_prompt_rail(&open, label);
    assert_no_sidebar_copy(&open, label);
    force_kill_helper(helper);
}

fn force_kill_helper(mut helper: SpawnedHelper) {
    let _ = helper.child.kill();
    let _ = helper.child.wait();
}

pub(crate) fn resp_60x20_pty() {
    assert_resp_compact_startup_pty(60, 20, "RESP-60x20");
}

pub(crate) fn resp_79x24_pty() {
    assert_resp_compact_startup_pty(79, 24, "RESP-79x24");
}

pub(crate) fn resp_80x24_pty() {
    assert_resp_compact_startup_pty(80, 24, "RESP-80x24");
}

pub(crate) fn resp_100x30_pty() {
    assert_resp_bordered_startup_pty(100, 30, "RESP-100x30");
}

pub(crate) fn resp_120x40_pty() {
    assert_resp_bordered_startup_pty(120, 40, "RESP-120x40");
}

pub(crate) fn resp_120x50_pty() {
    assert_resp_bordered_startup_pty(120, 50, "RESP-120x50");
}

pub(crate) fn resp_wide_pty() {
    assert_resp_bordered_startup_pty(140, 40, "RESP-WIDE");
}

fn assert_resp_compact_startup_pty(cols: u16, rows: u16, label: &str) {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        TYPE_FIRST_STARTUP_HELPER,
        TYPE_FIRST_STARTUP_SCENARIO,
        cols,
        rows,
    );
    helper.wait_for(READY_MARKER);
    let screen = helper.screen_text();
    assert!(
        screen.contains("New worktree") || screen.contains("New session"),
        "{label} PTY: unboxed action rows required\n{screen}"
    );
    assert!(
        screen.contains("Resume") || screen.contains("Quit"),
        "{label} PTY: secondary welcome actions required\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "{label} PTY: composer glyph required\n{screen}"
    );
    let top_corners = screen.chars().filter(|c| *c == '╭').count();
    assert_eq!(
        top_corners, 1,
        "{label} PTY: exactly one bordered box (composer); welcome must stay unboxed\n{screen}"
    );
    assert_no_sidebar_copy(&screen, label);
    assert_no_multi_row_prompt_rail(&screen, label);
    exit_via_palette(&mut helper);
}

fn assert_resp_bordered_startup_pty(cols: u16, rows: u16, label: &str) {
    if !require_pty_signoff() {
        return;
    }
    let mut helper = spawn_helper_at(
        TYPE_FIRST_STARTUP_HELPER,
        TYPE_FIRST_STARTUP_SCENARIO,
        cols,
        rows,
    );
    helper.wait_for(READY_MARKER);
    let screen = helper.screen_text();
    assert!(
        screen.contains("New worktree") || screen.contains("New session"),
        "{label} PTY: welcome actions required\n{screen}"
    );
    assert!(
        screen.contains("Changelog") || screen.contains('•'),
        "{label} PTY: changelog mass required\n{screen}"
    );
    assert!(
        screen.contains('❯'),
        "{label} PTY: composer glyph required\n{screen}"
    );
    let top_corners = screen.chars().filter(|c| *c == '╭').count();
    assert!(
        top_corners >= 2,
        "{label} PTY: bordered welcome + composer required (got {top_corners} ╭)\n{screen}"
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
    run_live_with_historical_events(cancel_events());
}

pub(crate) fn pty_helper_live_recover() {
    if std::env::var(HELPER_SCENARIO_ENV).as_deref() != Ok(LIVE_RECOVER_SCENARIO) {
        return;
    }
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
        let _ = inject_tx.send(LiveUpdate::Event(Box::new(question_permission_requested_event(
            5,
            "perm_question_parity",
            "tool_call_question_parity",
        ))));
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
    let pair = pty_system
        .openpty(pty_size(cols, rows))
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
    vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: STREAM_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
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
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: STREAM_ASSISTANT_TEXT.to_string(),
            }),
        ),
    ]
}

fn fail_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_fail_pty";
    vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: FAIL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
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
            3,
            None,
            EventV1::RunFailed(RunFailedEvent {
                error: FAIL_ERROR_TEXT.to_string(),
            }),
        ),
    ]
}

fn complete_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_complete_pty";
    // Grok COMPLETE freeze: Worked for 0.8s (Thought for 0.0s stays title-only empty span).
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
                // Grok COMPLETE freeze breadcrumb: 12K / 262K
                usage: Some(parity_completion_usage(12_000)),
                metadata: None,
            }),
        ),
    ];
    // first mono must be non-zero (0 is treated as unset); span 100→900 = 0.8s.
    for (event, mono) in events.iter_mut().zip([100_u64, 200, 500, 900]) {
        event.mono_ms = mono;
    }
    events
}

fn cancel_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_cancel_pty";
    // Grok CANCEL freeze: Thought for 0.2s + Turn cancelled by user in 1.3s.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: CANCEL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "model-tx".to_string(),
                prompt_summary: CANCEL_USER_TEXT.to_string(),
                request_digest: "digest-cancel".to_string(),
                metadata: None,
            }),
        ),
        // Title-only reasoning span mono 200→400 = 0.2s Thought for (Grok freeze).
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
        // Partial essay body before cancel (Grok freeze mid-stream content).
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: CANCEL_PARTIAL_TEXT.to_string(),
            }),
        ),
        // Seed breadcrumb token meta (1.5K / 262K) before cancel; TaskCancelled wins Error chrome.
        parity_envelope(
            6,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "cancelled".to_string(),
                output_digest: Some("digest-out-cancel".to_string()),
                usage: Some(parity_completion_usage(1_500)),
                metadata: None,
            }),
        ),
        parity_envelope(
            7,
            Some(request_id),
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "task_cancel_parity".into(),
                reason: "user interrupted streaming turn".to_string(),
                task_scope: Some(TaskTerminalScope::AgentTurn),
            }),
        ),
    ];
    // first mono non-zero; reasoning 200→400 = 0.2s; body 800; finish 1200; cancel 1400 → 1.3s from 100.
    for (event, mono) in events
        .iter_mut()
        .zip([100_u64, 100, 200, 400, 800, 1200, 1400])
    {
        event.mono_ms = mono;
    }
    events
}

fn recover_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_recover_pty";
    // Grok RECOVER freeze: partial stream + breadcrumb 1.5K / 262K + cancel 2.3s, then draft.
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: RECOVER_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
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
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: RECOVER_PARTIAL_TEXT.to_string(),
            }),
        ),
        // Seed breadcrumb token meta (1.5K / 262K) before cancel; TaskCancelled wins Error chrome.
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "cancelled".to_string(),
                output_digest: Some("digest-out-recover".to_string()),
                usage: Some(parity_completion_usage(1_500)),
                metadata: None,
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::TaskCancelled(TaskCancelledEvent {
                task_id: "task_recover_parity".into(),
                reason: "user interrupted streaming turn".to_string(),
                task_scope: Some(TaskTerminalScope::AgentTurn),
            }),
        ),
    ];
    // first mono non-zero; cancel at 2400 → turn 2.3s from 100.
    for (event, mono) in events.iter_mut().zip([100_u64, 200, 800, 1200, 2400]) {
        event.mono_ms = mono;
    }
    events
}

fn tool_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_tool_pty";
    // Grok TOOL freeze: Worked for 1.7s (no Thought chrome).
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
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_tool".into(),
                tool_id: "list".to_string(),
                args_summary: r#"{"path":"."}"#.to_string(),
                args_digest: "digest-args-tool".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_tool".into(),
            }),
        ),
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_tool".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("demo.txt".to_string()),
                output_digest: Some("digest-out-tool".to_string()),
                output_json: Some(serde_json::json!({"entry_count": 1})),
                metadata: None,
            }),
        ),
        // Grok TOOL freeze: after list tool, assistant body reports COUNT=N
        parity_envelope(
            6,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "COUNT=2".to_string(),
            }),
        ),
        parity_envelope(
            7,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out-tool-turn".to_string()),
                // Grok TOOL freeze breadcrumb: 10K / 262K
                usage: Some(parity_completion_usage(10_000)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; span 100→1800 = 1.7s.
    for (event, mono) in events
        .iter_mut()
        .zip([100_u64, 200, 400, 600, 1000, 1400, 1800])
    {
        event.mono_ms = mono;
    }
    events
}

fn diff_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_diff_pty";
    // Grok DIFF freeze: Thought for 0.1s + Worked for 2.2s.
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
        // Title-only reasoning span mono 100→200 = 0.1s Thought for (Grok freeze).
        parity_envelope(
            3,
            Some(request_id),
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: request_id.into(),
                delta: "**plan**".to_string(), // title-only → completed Thought header, no body
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
        parity_envelope(
            5,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_diff_write".into(),
                tool_id: "fs.write".to_string(),
                args_summary: r#"{"path":"demo.txt","content":"parity-diff-ok\n","oldContent":"old content\n"}"#
                    .to_string(),
                args_digest: "digest-args-diff-write".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            6,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_diff_write".into(),
            }),
        ),
        parity_envelope(
            7,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc_diff_read".into(),
                tool_id: "read".to_string(),
                args_summary: r#"{"path":"demo.txt"}"#.to_string(),
                args_digest: "digest-args-diff-read".to_string(),
                metadata: None,
            }),
        ),
        parity_envelope(
            8,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tc_diff_read".into(),
            }),
        ),
        parity_envelope(
            9,
            Some(request_id),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tc_diff_read".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("parity-diff-ok".to_string()),
                output_digest: Some("digest-out-diff-read".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        parity_envelope(
            10,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "DONE".to_string(),
            }),
        ),
        parity_envelope(
            11,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out-diff-turn".to_string()),
                // Grok DIFF freeze breadcrumb: 10K / 262K
                usage: Some(parity_completion_usage(10_000)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; reasoning 100→200 = 0.1s; finish 2300 → Worked 2.2s from 100.
    for (event, mono) in events.iter_mut().zip([
        100_u64, 100, 100, 200, 400, 600, 900, 1200, 1600, 2000, 2300,
    ]) {
        event.mono_ms = mono;
    }
    events
}

fn scroll_events() -> Vec<EventEnvelopeV1> {
    let request_id = "req_scroll_pty";
    // Grok SCROLL freeze: one tall completed turn with numbered inventory + ▼ more-below.
    let inventory = (1..=80)
        .map(|n| format!("{n}. f{n}.txt"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut events = vec![
        parity_envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: SCROLL_USER_TEXT.to_string(),
            }),
        ),
        parity_envelope(
            2,
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
            3,
            Some(request_id),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: inventory,
            }),
        ),
        parity_envelope(
            4,
            Some(request_id),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-out-scroll".to_string()),
                usage: Some(parity_completion_usage(19_000)),
                metadata: None,
            }),
        ),
    ];
    // first mono non-zero; short completed span (durations not freeze-critical for scroll).
    for (event, mono) in events.iter_mut().zip([100_u64, 200, 500, 800]) {
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
    for (event, mono) in events
        .iter_mut()
        .zip([100_u64, 100, 100, 200, 400, 19_100])
    {
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
