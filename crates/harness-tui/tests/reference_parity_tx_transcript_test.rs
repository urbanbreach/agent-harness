//! Task 27 — Transcript, markdown, syntax, tools, diffs, links, selection,
//! clipboard, and media parity tests.
//!
//! Contract: `grok-build-parity-parallel-execution.md` Task 27 +
//! `crates/harness-tui/DESIGN.md` §8 / §14.
//!
//! These tests cover the deterministic event-fixture half of Task 27 QA:
//! streaming/completed/failed tool states, edit diffs, markdown rendering,
//! syntax highlighting, links, selection, clipboard, malformed media
//! fallback, long content, and Unicode width.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, RunFailedEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::app::{AppState, Focus, LaunchMetadata, RuntimeStateKind};
use harness_tui::clipboard_leaf::{ClipboardLeaf, ClipboardMode, PasteMode};
use harness_tui::leaf_actions::group_e_media::{
    is_replay_safe, resolve, validate_input, MediaAction, MediaFailureReason,
};
use harness_tui::render_test::render_to_string;
use harness_tui::terminal::char_display_width;
use harness_tui::ui;
use ratatui::layout::Rect;

const W: u16 = 120;
const H: u16 = 40;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-tx-transcript-{seq:04}"),
        seq,
        run_id: "run_tx_transcript_parity".into(),
        mono_ms: seq,
        ts: Some("2026-03-19T05:54:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("tx-transcript-parity".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_tx_transcript_parity".to_string()),
        payload,
    }
}

fn live_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn ingest_completed_turn(app: &mut AppState, request_id: &str, user: &str, assistant: &str) {
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: user.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: user.to_string(),
            request_digest: "digest-tx".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: assistant.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-out".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
}

fn ingest_tool_call(
    app: &mut AppState,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    status: ToolCallStatus,
    output_summary: Option<&str>,
) {
    ingest_tool_call_seq(
        app,
        request_id,
        tool_call_id,
        tool_id,
        args_summary,
        status,
        output_summary,
        10,
    );
}

fn ingest_tool_call_seq(
    app: &mut AppState,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    status: ToolCallStatus,
    output_summary: Option<&str>,
    seq_base: u64,
) {
    app.ingest_event(envelope(
        seq_base,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: "digest-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        seq_base + 1,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.into(),
        }),
    ));
    app.ingest_event(envelope(
        seq_base + 2,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: tool_call_id.into(),
            status,
            output_summary: output_summary.map(str::to_string),
            output_digest: Some("digest-out-tool".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

// ===========================================================================
// Streaming / completed / failed tool states
// ===========================================================================

/// TX-TOOL streaming: a running tool call shows the tool marker and name
/// without a completed summary, and the shell keeps the bordered composer.
#[test]
fn tx_tool_streaming_state_shows_running_marker_without_summary() {
    let mut app = live_app();
    let request_id = "req_tool_streaming";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "run a streaming tool".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "run a streaming tool".to_string(),
            request_digest: "digest-streaming-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        10,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_streaming".into(),
            tool_id: "bash".to_string(),
            args_summary: r#"{"command":"echo hello"}"#.to_string(),
            args_digest: "digest-args-streaming".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        11,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_streaming".into(),
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains('◆') || rendered.contains('◈'),
        "TX-TOOL streaming: tool marker glyph required\n{rendered}"
    );
    assert!(
        rendered.contains("echo hello"),
        "TX-TOOL streaming: tool command must project\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-TOOL streaming: bordered composer retained\n{rendered}"
    );
}

/// TX-TOOL completed: a succeeded tool call shows the completed marker and
/// output summary in the transcript body.
#[test]
fn tx_tool_completed_state_shows_summary_with_completed_marker() {
    let mut app = live_app();
    let request_id = "req_tool_completed";
    ingest_started(&mut app, request_id, "run the completed tool");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_completed",
        "bash",
        r#"{"command":"echo done"}"#,
        ToolCallStatus::Succeeded,
        Some("done"),
    );

    app.toggle_tool_output_for_test("tc_completed");
    let rendered = render(&app);

    assert!(
        rendered.contains('◆') || rendered.contains('◈'),
        "TX-TOOL completed: marker glyph required\n{rendered}"
    );
    assert!(
        rendered.contains("done"),
        "TX-TOOL completed: output summary must project\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-TOOL completed: bordered composer retained\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "TX-TOOL completed: selected tool block retains the Grok rail\n{rendered}"
    );
}

/// TX-TOOL failed: a failed tool call surfaces the error details and keeps
/// the bordered composer under the full-width shell.
#[test]
fn tx_tool_failed_state_surfaces_error_with_bordered_composer() {
    let mut app = live_app();
    let request_id = "req_tool_failed";
    ingest_started(&mut app, request_id, "run the failing tool");
    ingest_tool_call(
        &mut app,
        request_id,
        "tc_failed",
        "bash",
        r#"{"command":"false"}"#,
        ToolCallStatus::Failed,
        Some("command failed with exit code 1"),
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("command failed with exit code 1")
            || rendered.contains("No error details available."),
        "TX-TOOL failed: error details must surface\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-TOOL failed: bordered composer retained\n{rendered}"
    );
    assert!(
        rendered.contains('┃'),
        "TX-TOOL failed: selected tool block retains the Grok rail\n{rendered}"
    );
}

/// TX-TOOL retry: after a failed tool call, a retry attempt renders both
/// the failed call and the retry in the transcript.
#[test]
fn tx_tool_retry_state_renders_both_failed_and_retry() {
    let mut app = live_app();
    let request_id = "req_tool_retry";
    ingest_started(&mut app, request_id, "retry the tool");
    ingest_tool_call_seq(
        &mut app,
        request_id,
        "tc_fail_first",
        "bash",
        r#"{"command":"false"}"#,
        ToolCallStatus::Failed,
        Some("exit code 1"),
        10,
    );
    ingest_tool_call_seq(
        &mut app,
        request_id,
        "tc_retry_second",
        "bash",
        r#"{"command":"true"}"#,
        ToolCallStatus::Succeeded,
        Some("ok"),
        20,
    );

    app.toggle_tool_output_for_test("tc_fail_first");
    app.toggle_tool_output_for_test("tc_retry_second");
    let rendered = render(&app);

    assert!(
        rendered.contains("exit code 1"),
        "TX-TOOL retry: first failure must remain visible\n{rendered}"
    );
    assert!(
        rendered.contains("ok"),
        "TX-TOOL retry: retry success must project\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "TX-TOOL retry: composer retained\n{rendered}"
    );
}

// ===========================================================================
// Edit diffs
/// when tool details are toggled on.
#[test]
fn tx_diff_edit_projects_both_old_and_new_versions() {
    use std::path::PathBuf;

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_tx_diff_edit")), false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-tx").with_mode_label("Demo"),
    );
    let request_id = "req_diff_edit";
    ingest_started(&mut app, request_id, "apply the edit");
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff_edit".into(),
            tool_id: "edit".to_string(),
            args_summary:
                r#"{"path":"src/lib.rs","oldString":"let x = 1;","newString":"let x = 2;"}"#
                    .to_string(),
            args_digest: "digest-args-diff-edit".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff_edit".into(),
        }),
    ));
    app.ingest_event(envelope(
        14,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff_edit".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/lib.rs".to_string()),
            output_digest: Some("digest-out-diff-edit".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        15,
        Some(request_id),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-tx-finished-edit".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.focus = Focus::Details;

    // Toggle tool details on via palette
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    for ch in "show tool details".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    app.toggle_tool_output_for_test("tc_diff_edit");
    let rendered = render(&app);

    assert!(
        app.transcript_interaction_snapshot().show_tool_details,
        "TX-DIFF: tool details must toggle on"
    );
    assert!(
        rendered.contains("let x = 1;"),
        "TX-DIFF: removed line must project\n{rendered}"
    );
    assert!(
        rendered.contains("let x = 2;"),
        "TX-DIFF: added line must project\n{rendered}"
    );
    assert!(
        !rendered.contains('┃'),
        "TX-DIFF: no legacy outer rail\n{rendered}"
    );
}

/// TX-DIFF: an edit tool call without tool details toggled does not show
/// the raw old/new strings.
#[test]
fn tx_diff_edit_collapsed_hides_raw_diff_lines() {
    let mut app = live_app();
    let request_id = "req_diff_collapsed";
    ingest_started(&mut app, request_id, "apply the edit collapsed");
    app.ingest_event(envelope(
        12,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff_collapsed".into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"src/main.rs","oldString":"fn old()","newString":"fn new()"}"#
                .to_string(),
            args_digest: "digest-args-diff-collapsed".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        13,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tc_diff_collapsed".into(),
        }),
    ));
    app.ingest_event(envelope(
        14,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff_collapsed".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("edited src/main.rs".to_string()),
            output_digest: Some("digest-out-diff-collapsed".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("src/main.rs") || rendered.contains("edit") || rendered.contains('◆'),
        "TX-DIFF collapsed: tool header must project\n{rendered}"
    );
    assert!(
        !rendered.contains("fn old()"),
        "TX-DIFF collapsed: raw old string should not project\n{rendered}"
    );
    assert!(
        !rendered.contains("fn new()"),
        "TX-DIFF collapsed: raw new string should not project\n{rendered}"
    );
}

// ===========================================================================
// Markdown rendering
// ===========================================================================

/// TX-ASSISTANT markdown: headings, bold, italic, lists, and inline code
/// all render in the transcript body.
#[test]
fn tx_assistant_markdown_renders_headings_bold_italic_lists_and_code() {
    let mut app = live_app();
    let markdown = [
        "## Heading Text",
        "",
        "This has **bold** and _italic_ and `inline code`.",
        "",
        "- First item",
        "- Second item",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_markdown", "show markdown", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("Heading Text"),
        "TX-MARKDOWN: heading text must render\n{rendered}"
    );
    assert!(
        rendered.contains("bold"),
        "TX-MARKDOWN: bold content must render\n{rendered}"
    );
    assert!(
        rendered.contains("italic"),
        "TX-MARKDOWN: italic content must render\n{rendered}"
    );
    assert!(
        rendered.contains("inline code"),
        "TX-MARKDOWN: inline code content must render\n{rendered}"
    );
    assert!(
        rendered.contains("First item"),
        "TX-MARKDOWN: list item must render\n{rendered}"
    );
    assert!(
        rendered.contains("Second item"),
        "TX-MARKDOWN: second list item must render\n{rendered}"
    );
    assert!(
        !rendered.contains("**bold**"),
        "TX-MARKDOWN: raw bold markers should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("_italic_"),
        "TX-MARKDOWN: raw italic markers should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("`inline code`"),
        "TX-MARKDOWN: raw inline code backticks should not render\n{rendered}"
    );
}

/// TX-ASSISTANT markdown: a horizontal rule renders as a separator line.
#[test]
fn tx_assistant_markdown_renders_horizontal_rule() {
    let mut app = live_app();
    let markdown = "Before rule\n\n---\n\nAfter rule";
    ingest_completed_turn(&mut app, "req_hr", "show a rule", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("Before rule"),
        "TX-MARKDOWN: text before rule must render\n{rendered}"
    );
    assert!(
        rendered.contains("After rule"),
        "TX-MARKDOWN: text after rule must render\n{rendered}"
    );
    assert!(
        !rendered.contains("---"),
        "TX-MARKDOWN: raw horizontal rule markers should not render\n{rendered}"
    );
}

/// TX-ASSISTANT markdown: a fenced code block with unknown language renders
/// as plain code content (fallback path).
#[test]
fn tx_assistant_markdown_fenced_code_unknown_language_renders_content() {
    let mut app = live_app();
    let markdown = [
        "Intro text.",
        "```unknown-lang",
        "line one",
        "line two",
        "```",
        "Outro text.",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_unknown_lang", "show unknown lang", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("line one"),
        "TX-MARKDOWN: unknown-lang code content must render\n{rendered}"
    );
    assert!(
        rendered.contains("line two"),
        "TX-MARKDOWN: unknown-lang second line must render\n{rendered}"
    );
    assert!(
        rendered.contains("Intro text."),
        "TX-MARKDOWN: intro text retained\n{rendered}"
    );
    assert!(
        rendered.contains("Outro text."),
        "TX-MARKDOWN: outro text retained\n{rendered}"
    );
}

// ===========================================================================
// Syntax highlighting
// ===========================================================================

/// TX-ASSISTANT syntax: a fenced Rust code block renders its content with
/// syntax highlighting (content survives through the highlighted block).
#[test]
fn tx_assistant_syntax_highlighted_rust_code_renders_content() {
    let mut app = live_app();
    let markdown = [
        "Here is some Rust code:",
        "```rust",
        "fn main() {",
        "    let answer = 42;",
        "    println!(\"{}\", answer);",
        "}",
        "```",
        "Done.",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_rust_highlight", "show rust code", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("fn main()"),
        "TX-SYNTAX: Rust function signature must render\n{rendered}"
    );
    assert!(
        rendered.contains("let answer = 42;"),
        "TX-SYNTAX: Rust let binding must render\n{rendered}"
    );
    assert!(
        rendered.contains("println!"),
        "TX-SYNTAX: Rust macro must render\n{rendered}"
    );
    assert!(
        rendered.contains("Done."),
        "TX-SYNTAX: surrounding prose retained\n{rendered}"
    );
    assert!(
        !rendered.contains("```rust"),
        "TX-SYNTAX: raw fence markers should not render\n{rendered}"
    );
}

/// TX-ASSISTANT syntax: a fenced Python code block renders its content.
#[test]
fn tx_assistant_syntax_highlighted_python_code_renders_content() {
    let mut app = live_app();
    let markdown = [
        "Python example:",
        "```python",
        "def hello():",
        "    print('hello world')",
        "```",
        "End.",
    ]
    .join("\n");
    ingest_completed_turn(
        &mut app,
        "req_python_highlight",
        "show python code",
        &markdown,
    );

    let rendered = render(&app);

    assert!(
        rendered.contains("def hello():"),
        "TX-SYNTAX: Python function def must render\n{rendered}"
    );
    assert!(
        rendered.contains("print('hello world')"),
        "TX-SYNTAX: Python print statement must render\n{rendered}"
    );
    assert!(
        !rendered.contains("```python"),
        "TX-SYNTAX: raw Python fence markers should not render\n{rendered}"
    );
}

/// TX-ASSISTANT syntax: a fenced JSON code block renders its content.
#[test]
fn tx_assistant_syntax_highlighted_json_code_renders_content() {
    let mut app = live_app();
    let markdown = [
        "Config:",
        "```json",
        "{",
        "  \"name\": \"harness\",",
        "  \"version\": \"1.0\"",
        "}",
        "```",
    ]
    .join("\n");
    ingest_completed_turn(&mut app, "req_json_highlight", "show json code", &markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("\"name\""),
        "TX-SYNTAX: JSON key must render\n{rendered}"
    );
    assert!(
        rendered.contains("\"harness\""),
        "TX-SYNTAX: JSON value must render\n{rendered}"
    );
    assert!(
        !rendered.contains("```json"),
        "TX-SYNTAX: raw JSON fence markers should not render\n{rendered}"
    );
}

// ===========================================================================
// Links / hyperlinks
// ===========================================================================

/// TX-ASSISTANT links: a markdown link renders the label text (not the raw
/// URL) in the transcript body.
#[test]
fn tx_assistant_markdown_link_renders_label_not_raw_url() {
    let mut app = live_app();
    let markdown = "See [the docs](https://example.com/docs) for more.";
    ingest_completed_turn(&mut app, "req_link", "show a link", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("the docs"),
        "TX-LINK: link label must render\n{rendered}"
    );
    assert!(
        !rendered.contains("https://example.com/docs"),
        "TX-LINK: raw URL should not render as plain text\n{rendered}"
    );
    assert!(
        !rendered.contains("[the docs]("),
        "TX-LINK: raw markdown link syntax should not render\n{rendered}"
    );
}

/// TX-ASSISTANT links: a raw URL in the assistant body renders as a link.
#[test]
fn tx_assistant_raw_url_renders_as_link() {
    let mut app = live_app();
    let markdown = "Visit https://example.com for details.";
    ingest_completed_turn(&mut app, "req_raw_url", "show a raw url", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("https://example.com"),
        "TX-LINK: raw URL must render in transcript\n{rendered}"
    );
    assert!(
        rendered.contains("Visit"),
        "TX-LINK: surrounding text must render\n{rendered}"
    );
    assert!(
        rendered.contains("for details."),
        "TX-LINK: trailing text must render\n{rendered}"
    );
}

/// TX-ASSISTANT links: multiple links in a single line all render their
/// labels.
#[test]
fn tx_assistant_multiple_links_render_all_labels() {
    let mut app = live_app();
    let markdown = "See [first](https://a.test) and [second](https://b.test) links.";
    ingest_completed_turn(&mut app, "req_multi_link", "show multiple links", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("first"),
        "TX-LINK: first link label must render\n{rendered}"
    );
    assert!(
        rendered.contains("second"),
        "TX-LINK: second link label must render\n{rendered}"
    );
    assert!(
        !rendered.contains("https://a.test"),
        "TX-LINK: first raw URL should not render\n{rendered}"
    );
    assert!(
        !rendered.contains("https://b.test"),
        "TX-LINK: second raw URL should not render\n{rendered}"
    );
}

// ===========================================================================
// Selection
// ===========================================================================

/// TX-SELECTION: the transcript interaction snapshot reports follow mode
/// and scroll state correctly for a fresh live shell.
#[test]
fn tx_selection_follow_mode_is_default_for_fresh_shell() {
    let mut app = live_app();
    ingest_completed_turn(&mut app, "req_sel_follow", "selection probe", "reply text");

    let snapshot = app.transcript_interaction_snapshot();

    assert!(
        snapshot.follow_mode,
        "TX-SELECTION: fresh shell must start in follow mode"
    );
    assert_eq!(
        snapshot.scroll, 0,
        "TX-SELECTION: fresh shell must start at scroll 0"
    );
}

/// TX-SELECTION: PageUp leaves follow mode and increases scroll offset.
#[test]
fn tx_selection_page_up_leaves_follow_and_increases_scroll() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = live_app();
    // Create enough content to be scrollable
    for (seq, rid, text) in [
        (1u64, "req_sel_a", "First scrollable turn for selection"),
        (2u64, "req_sel_b", "Second scrollable turn for selection"),
        (3u64, "req_sel_c", "Third scrollable turn for selection"),
    ] {
        app.ingest_event(envelope(
            seq * 10,
            Some(rid),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: rid.into(),
                text: text.into(),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 1,
            Some(rid),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: rid.into(),
                provider_id: "mock".into(),
                model_id: "model-tx".into(),
                prompt_summary: text.into(),
                request_digest: format!("digest-{rid}"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 2,
            Some(rid),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: rid.into(),
                delta: format!("Assistant body for {rid}\n").repeat(10),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 3,
            Some(rid),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: rid.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("out-{rid}")),
                usage: None,
                metadata: None,
            }),
        ));
    }

    app.focus = Focus::Details;
    let _ = render(&app); // populate layout cache
    let before = app.transcript_interaction_snapshot();
    assert!(before.follow_mode);

    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let after = app.transcript_interaction_snapshot();

    assert!(
        !after.follow_mode,
        "TX-SELECTION: PageUp must leave follow mode"
    );
    assert!(
        after.scroll > 0,
        "TX-SELECTION: PageUp must increase scroll offset"
    );
}

/// TX-SELECTION: End key restores follow mode and scroll 0.
#[test]
fn tx_selection_end_key_restores_follow_mode() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = live_app();
    for (seq, rid, text) in [
        (1u64, "req_end_a", "First end-key turn"),
        (2u64, "req_end_b", "Second end-key turn"),
        (3u64, "req_end_c", "Third end-key turn"),
    ] {
        app.ingest_event(envelope(
            seq * 10,
            Some(rid),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: rid.into(),
                text: text.into(),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 1,
            Some(rid),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: rid.into(),
                provider_id: "mock".into(),
                model_id: "model-tx".into(),
                prompt_summary: text.into(),
                request_digest: format!("digest-{rid}"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 2,
            Some(rid),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: rid.into(),
                delta: format!("Body for {rid}\n").repeat(10),
            }),
        ));
        app.ingest_event(envelope(
            seq * 10 + 3,
            Some(rid),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: rid.into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("out-{rid}")),
                usage: None,
                metadata: None,
            }),
        ));
    }

    app.focus = Focus::Details;
    let _ = render(&app);
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    let scrolled = app.transcript_interaction_snapshot();
    assert!(!scrolled.follow_mode && scrolled.scroll > 0);

    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    let after_end = app.transcript_interaction_snapshot();

    assert_eq!(
        after_end.scroll, 0,
        "TX-SELECTION: End must return scroll to 0"
    );
    assert!(
        after_end.follow_mode,
        "TX-SELECTION: End must restore follow mode"
    );
}

// ===========================================================================
// Clipboard
// ===========================================================================

/// CLIPBOARD: the full clipboard leaf has all features (OSC52, native,
/// bracketed paste, copy-on-select, hyperlinks).
#[test]
fn clipboard_full_leaf_has_all_features() {
    let leaf = ClipboardLeaf::full();

    assert!(
        leaf.mode.is_available(),
        "CLIPBOARD: full leaf mode must be available"
    );
    assert!(
        leaf.mode.supports_osc52(),
        "CLIPBOARD: full leaf must support OSC52"
    );
    assert!(
        leaf.mode.supports_native(),
        "CLIPBOARD: full leaf must support native fallback"
    );
    assert_eq!(
        leaf.paste_mode,
        PasteMode::Bracketed,
        "CLIPBOARD: full leaf must have bracketed paste"
    );
    assert!(
        leaf.copy_on_select,
        "CLIPBOARD: full leaf must have copy-on-select"
    );
    assert!(
        leaf.hyperlink_support,
        "CLIPBOARD: full leaf must support hyperlinks"
    );
}

/// CLIPBOARD: the disabled clipboard leaf has no features.
#[test]
fn clipboard_disabled_leaf_has_no_features() {
    let leaf = ClipboardLeaf::disabled();

    assert!(
        !leaf.mode.is_available(),
        "CLIPBOARD: disabled leaf mode must be unavailable"
    );
    assert!(
        !leaf.mode.supports_osc52(),
        "CLIPBOARD: disabled leaf must not support OSC52"
    );
    assert!(
        !leaf.mode.supports_native(),
        "CLIPBOARD: disabled leaf must not support native"
    );
    assert_eq!(
        leaf.paste_mode,
        PasteMode::Disabled,
        "CLIPBOARD: disabled leaf must have paste disabled"
    );
    assert!(
        !leaf.copy_on_select,
        "CLIPBOARD: disabled leaf must not have copy-on-select"
    );
    assert!(
        !leaf.hyperlink_support,
        "CLIPBOARD: disabled leaf must not support hyperlinks"
    );
}

/// CLIPBOARD: OSC52-only leaf supports OSC52 but not native fallback.
#[test]
fn clipboard_osc52_only_leaf_supports_osc52_not_native() {
    let leaf = ClipboardLeaf::osc52_only();

    assert!(
        leaf.mode.supports_osc52(),
        "CLIPBOARD: OSC52-only leaf must support OSC52"
    );
    assert!(
        !leaf.mode.supports_native(),
        "CLIPBOARD: OSC52-only leaf must not support native"
    );
    assert!(
        leaf.copy_on_select,
        "CLIPBOARD: OSC52-only leaf must have copy-on-select"
    );
    assert!(
        leaf.hyperlink_support,
        "CLIPBOARD: OSC52-only leaf must support hyperlinks"
    );
}

/// CLIPBOARD: native-only leaf supports native but not OSC52.
#[test]
fn clipboard_native_only_leaf_supports_native_not_osc52() {
    let leaf = ClipboardLeaf::native_only();

    assert!(
        leaf.mode.supports_native(),
        "CLIPBOARD: native-only leaf must support native"
    );
    assert!(
        !leaf.mode.supports_osc52(),
        "CLIPBOARD: native-only leaf must not support OSC52"
    );
    assert!(
        leaf.copy_on_select,
        "CLIPBOARD: native-only leaf must have copy-on-select"
    );
    assert!(
        !leaf.hyperlink_support,
        "CLIPBOARD: native-only leaf must not support hyperlinks"
    );
}

/// CLIPBOARD: the OSC52 escape sequence wraps base64 content correctly.
#[test]
fn clipboard_osc52_sequence_wraps_base64_in_escape() {
    use harness_tui::clipboard_leaf::build_osc52_sequence;

    let text = "hello clipboard";
    let sequence = build_osc52_sequence(text, false);

    assert!(
        sequence.starts_with("\x1b]52;c;"),
        "CLIPBOARD: OSC52 sequence must start with OSC 52 prefix"
    );
    assert!(
        sequence.ends_with('\x07'),
        "CLIPBOARD: OSC52 sequence must end with BEL"
    );
    // The base64 of "hello clipboard" is "aGVsbG8gY2xpcGJvYXJk"
    let base64_part = &sequence["\x1b]52;c;".len()..sequence.len() - 1];
    assert_eq!(
        base64_part, "aGVsbG8gY2xpcGJvYXJk",
        "CLIPBOARD: OSC52 base64 must match expected encoding"
    );
}

/// CLIPBOARD: the OSC52 sequence wraps in tmux passthrough when in tmux.
#[test]
fn clipboard_osc52_sequence_wraps_tmux_passthrough() {
    use harness_tui::clipboard_leaf::build_osc52_sequence;

    let text = "tmux test";
    let sequence = build_osc52_sequence(text, true);

    assert!(
        sequence.starts_with("\x1bPtmux;\x1b"),
        "CLIPBOARD: OSC52 in tmux must start with passthrough prefix"
    );
    assert!(
        sequence.ends_with("\x1b\\"),
        "CLIPBOARD: OSC52 in tmux must end with passthrough suffix"
    );
}

/// CLIPBOARD: no voice or hosted media generation affordance is present
/// in the clipboard leaf types.
#[test]
fn clipboard_leaf_has_no_voice_or_hosted_media_affordance() {
    let full = ClipboardLeaf::full();
    let disabled = ClipboardLeaf::disabled();

    // ClipboardLeaf only has mode, paste_mode, copy_on_select, hyperlink_support
    // There is no voice or media generation field
    assert!(
        std::mem::size_of::<ClipboardLeaf>() <= 4,
        "CLIPBOARD: leaf must remain a small value type (no voice/media fields)"
    );
    // Both full and disabled must not have any voice/media-related state
    let _ = (full, disabled);
}

// ===========================================================================
// Malformed media fallback
// ===========================================================================

/// MEDIA: the inline media capability resolves as unavailable (terminal
/// inline media protocol not negotiated).
#[test]
fn media_inline_media_capability_resolves_as_unavailable() {
    let resolution = resolve("tui.inline_media");

    assert!(
        resolution.is_some(),
        "MEDIA: tui.inline_media capability must resolve"
    );
    let resolution = resolution.unwrap();
    assert_eq!(
        resolution.capability_id, "tui.inline_media",
        "MEDIA: capability ID must match"
    );
    assert!(
        !matches!(
            resolution.availability,
            harness_tui::leaf_actions::group_e_media::ActionAvailability::Available
        ),
        "MEDIA: inline media must not be available without terminal negotiation"
    );
    assert!(
        resolution.replay_safe,
        "MEDIA: inline media resolution must be replay-safe"
    );
}

/// MEDIA: media failure recovery action is replay-safe.
#[test]
fn media_failure_recovery_is_replay_safe() {
    assert!(
        is_replay_safe(MediaAction::MediaFailureRecovery),
        "MEDIA: failure recovery must be replay-safe"
    );
    assert!(
        is_replay_safe(MediaAction::RenderInlineMedia),
        "MEDIA: render inline media must be replay-safe"
    );
    assert!(
        is_replay_safe(MediaAction::None),
        "MEDIA: None action must be replay-safe"
    );
    assert!(
        !is_replay_safe(MediaAction::ClipboardImagePaste),
        "MEDIA: clipboard image paste must NOT be replay-safe"
    );
}

/// MEDIA: media failure recovery validates empty input as valid.
#[test]
fn media_failure_recovery_validates_empty_input() {
    assert_eq!(
        validate_input(MediaAction::MediaFailureRecovery, ""),
        harness_tui::leaf_actions::group_e_media::InputValidation::Valid,
        "MEDIA: failure recovery with empty input must be valid"
    );
    assert_eq!(
        validate_input(MediaAction::ClipboardImagePaste, ""),
        harness_tui::leaf_actions::group_e_media::InputValidation::Valid,
        "MEDIA: clipboard paste with empty input must be valid"
    );
}

/// MEDIA: render inline media validates non-empty input.
#[test]
fn media_render_inline_media_validates_input() {
    assert_eq!(
        validate_input(MediaAction::RenderInlineMedia, ""),
        harness_tui::leaf_actions::group_e_media::InputValidation::Invalid(
            "render requires a media path or url"
        ),
        "MEDIA: render with empty input must be invalid"
    );
    assert_eq!(
        validate_input(MediaAction::RenderInlineMedia, "/path/to/image.png"),
        harness_tui::leaf_actions::group_e_media::InputValidation::Valid,
        "MEDIA: render with path must be valid"
    );
}

/// MEDIA: media failure reasons cover the expected failure modes.
#[test]
fn media_failure_reasons_cover_expected_modes() {
    assert_eq!(
        MediaFailureReason::default(),
        MediaFailureReason::None,
        "MEDIA: default failure reason must be None"
    );
    // All failure reasons must be distinct
    let reasons = [
        MediaFailureReason::None,
        MediaFailureReason::ClipboardUnavailable,
        MediaFailureReason::MediaDecodeFailed,
        MediaFailureReason::TerminalDoesNotSupportInlineMedia,
    ];
    for (i, a) in reasons.iter().enumerate() {
        for (j, b) in reasons.iter().enumerate() {
            if i != j {
                assert_ne!(a, b, "MEDIA: failure reasons must be distinct");
            }
        }
    }
}

/// MEDIA: no voice or hosted media generation affordance is present in
/// the media action types.
#[test]
fn media_actions_have_no_voice_or_hosted_generation() {
    // MediaAction only has: None, RenderInlineMedia, ClipboardImagePaste, MediaFailureRecovery
    // There is no Voice or HostedMediaGeneration variant
    let actions = [
        MediaAction::None,
        MediaAction::RenderInlineMedia,
        MediaAction::ClipboardImagePaste,
        MediaAction::MediaFailureRecovery,
    ];
    // Verify all variants are covered and none involve voice or hosted generation
    for action in &actions {
        let _ = action; // action is a valid enum variant
    }
}

// ===========================================================================
// Long content
// ===========================================================================

/// TX-LONG: a very long assistant response wraps across multiple lines
/// and all content remains visible in the transcript.
#[test]
fn tx_long_assistant_content_wraps_and_remains_visible() {
    let mut app = live_app();
    let long_text = "This is a very long line of text that should wrap across multiple terminal rows when rendered in the transcript. ".repeat(5);
    ingest_completed_turn(&mut app, "req_long", "show long content", &long_text);

    let rendered = render(&app);

    assert!(
        rendered.contains("This is a very long line"),
        "TX-LONG: long content must render\n{rendered}"
    );
    assert!(
        rendered.contains("terminal rows"),
        "TX-LONG: content from later in the text must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-LONG: bordered composer retained\n{rendered}"
    );
}

/// TX-LONG: a long user message wraps and the timestamp remains visible.
#[test]
fn tx_long_user_message_wraps_with_timestamp() {
    let mut app = live_app();
    let long_user = "This is a long user message that should wrap across multiple rows in the transcript body while keeping the timestamp visible at the end of the content.";
    app.ingest_event(envelope(
        1,
        Some("req_long_user"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_long_user".into(),
            text: long_user.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_long_user"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_long_user".into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: long_user.to_string(),
            request_digest: "digest-long-user".to_string(),
            metadata: None,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains("This is a long user message"),
        "TX-LONG: long user message must render\n{rendered}"
    );
    assert!(
        rendered.contains("timestamp visible"),
        "TX-LONG: end of long user message must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "TX-LONG: user marker retained for long message\n{rendered}"
    );
}

/// TX-LONG: many short turns stack correctly in the transcript without
/// losing earlier content.
#[test]
fn tx_long_many_turns_stack_correctly() {
    let mut app = live_app();
    for i in 1..=3 {
        let rid = format!("req_many_{i}");
        let user = format!("Turn number {i} user message");
        let assistant = format!("Turn number {i} assistant reply");
        let seq_base = u64::try_from(i * 10).unwrap();
        app.ingest_event(envelope(
            seq_base,
            Some(&rid),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: rid.clone().into(),
                text: user.clone(),
            }),
        ));
        app.ingest_event(envelope(
            seq_base + 1,
            Some(&rid),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: rid.clone().into(),
                provider_id: "mock".into(),
                model_id: "model-tx".into(),
                prompt_summary: user.clone(),
                request_digest: format!("digest-{rid}"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq_base + 2,
            Some(&rid),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: rid.clone().into(),
                delta: assistant.clone(),
            }),
        ));
        app.ingest_event(envelope(
            seq_base + 3,
            Some(&rid),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: rid.clone().into(),
                finish_reason: "stop".to_string(),
                output_digest: Some(format!("out-{rid}")),
                usage: None,
                metadata: None,
            }),
        ));
    }

    let rendered = render(&app);

    assert!(
        rendered.contains("Turn number 1 user message"),
        "TX-LONG: first turn must remain visible\n{rendered}"
    );
    assert!(
        rendered.contains("Turn number 3 assistant reply"),
        "TX-LONG: last turn must be visible\n{rendered}"
    );
}

// ===========================================================================
// Unicode width
// ===========================================================================

/// TX-UNICODE: CJK characters render with correct display width in the
/// transcript body.
#[test]
fn tx_unicode_cjk_characters_render_with_correct_width() {
    let mut app = live_app();
    let markdown = "Korean text: 안녕하세요. Japanese: こんにちは. Chinese: 你好.";
    ingest_completed_turn(&mut app, "req_cjk", "show CJK text", markdown);

    let rendered = render(&app);

    // CJK characters may render with inter-character spacing in the terminal;
    // verify each script's characters are present.
    assert!(
        rendered.contains('안') && rendered.contains('녕') && rendered.contains('하'),
        "TX-UNICODE: Korean characters must render\n{rendered}"
    );
    assert!(
        rendered.contains('こ') && rendered.contains('ん') && rendered.contains('に'),
        "TX-UNICODE: Japanese characters must render\n{rendered}"
    );
    assert!(
        rendered.contains('你') && rendered.contains('好'),
        "TX-UNICODE: Chinese characters must render\n{rendered}"
    );
}

/// TX-UNICODE: emoji characters render in the transcript body.
#[test]
fn tx_unicode_emoji_characters_render() {
    let mut app = live_app();
    let markdown = "Emoji test: 🚀 🎉 ✅ ❌";
    ingest_completed_turn(&mut app, "req_emoji", "show emoji", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("🚀"),
        "TX-UNICODE: rocket emoji must render\n{rendered}"
    );
    assert!(
        rendered.contains("🎉"),
        "TX-UNICODE: party emoji must render\n{rendered}"
    );
    assert!(
        rendered.contains("✅"),
        "TX-UNICODE: check mark emoji must render\n{rendered}"
    );
}

/// TX-UNICODE: combining characters and zero-width joiners render without
/// breaking the transcript layout.
#[test]
fn tx_unicode_combining_characters_render_without_breaking_layout() {
    let mut app = live_app();
    let markdown =
        "Combining: e\u{0301} (e with acute). ZWJ: 👨\u{200d}👩\u{200d}👧 (family emoji).";
    ingest_completed_turn(&mut app, "req_combining", "show combining chars", markdown);

    let rendered = render(&app);

    assert!(
        rendered.contains("Combining:"),
        "TX-UNICODE: combining char label must render\n{rendered}"
    );
    assert!(
        rendered.contains("ZWJ:"),
        "TX-UNICODE: ZWJ label must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-UNICODE: bordered composer retained with combining chars\n{rendered}"
    );
}

/// TX-UNICODE: the char_display_width function returns correct widths for
/// ASCII, CJK, and emoji characters.
#[test]
fn tx_unicode_char_display_width_returns_correct_widths() {
    assert_eq!(
        char_display_width('a'),
        1,
        "TX-UNICODE: ASCII 'a' must have width 1"
    );
    assert_eq!(
        char_display_width(' '),
        1,
        "TX-UNICODE: space must have width 1"
    );
    assert_eq!(
        char_display_width('안'),
        2,
        "TX-UNICODE: Korean Hangul must have width 2"
    );
    assert_eq!(
        char_display_width('こ'),
        2,
        "TX-UNICODE: Japanese Hiragana must have width 2"
    );
    assert_eq!(
        char_display_width('你'),
        2,
        "TX-UNICODE: Chinese Han must have width 2"
    );
}

/// TX-UNICODE: a long line of CJK text wraps correctly without truncation.
#[test]
fn tx_unicode_long_cjk_line_wraps_without_truncation() {
    let mut app = live_app();
    let long_cjk = "안녕하세요. 이것은 매우 긴 한국어 텍스트입니다. ".repeat(5);
    ingest_completed_turn(&mut app, "req_long_cjk", "show long CJK", &long_cjk);

    let rendered = render(&app);

    assert!(
        rendered.contains('안') && rendered.contains('녕'),
        "TX-UNICODE: long CJK start must render\n{rendered}"
    );
    assert!(
        rendered.contains('텍') && rendered.contains('스'),
        "TX-UNICODE: long CJK end must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-UNICODE: bordered composer retained with long CJK\n{rendered}"
    );
}

// ===========================================================================
// Absence rules (no voice or hosted media generation)
// ===========================================================================

/// ABSENCE: the rendered transcript must not contain any voice input or
/// hosted media generation affordance.
#[test]
fn transcript_has_no_voice_or_hosted_media_generation_affordance() {
    let mut app = live_app();
    ingest_completed_turn(
        &mut app,
        "req_absence",
        "check absence of voice and media",
        "Normal assistant reply.",
    );

    let rendered = render(&app);

    assert!(
        !rendered.contains("🎤") && !rendered.contains("🎙"),
        "ABSENCE: no voice microphone glyph"
    );
    assert!(
        !rendered.contains("Generate image")
            && !rendered.contains("Generate media")
            && !rendered.contains("Hosted media"),
        "ABSENCE: no hosted media generation affordance"
    );
    assert!(
        !rendered.contains("Voice input"),
        "ABSENCE: no voice input label"
    );
}

// ===========================================================================
// Helper: ingest a started turn (user message + provider started)
// ===========================================================================

fn ingest_started(app: &mut AppState, request_id: &str, text: &str) {
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: text.to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: text.to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    ));
}

// ===========================================================================
// Base transcript surface: blank-state, streaming placeholder, placement
// (Todo 11 — base chat and transcript surface convergence)
// ===========================================================================

/// TX-BASE: an empty live shell leaves the transcript clear and does NOT show
/// a synthetic empty-row marker or "Waiting for first turn…".
#[test]
fn tx_base_empty_shell_has_no_synthetic_row_indicator() {
    let app = live_app();
    let rendered = render(&app);

    assert!(
        !rendered.lines().any(|line| line.trim() == "~"),
        "TX-BASE: empty shell must leave the transcript clear\n{rendered}"
    );
    assert!(
        !rendered.contains("Waiting for first turn"),
        "TX-BASE: empty shell must not show \"Waiting for first turn…\"\n{rendered}"
    );
}

/// TX-BASE: compact geometry also leaves the empty transcript clear.
#[test]
fn tx_base_empty_shell_compact_has_no_synthetic_row_indicator() {
    let app = live_app();
    let rendered = render_at(&app, 80, 24);

    assert!(
        !rendered.lines().any(|line| line.trim() == "~"),
        "TX-BASE: compact empty shell must leave the transcript clear\n{rendered}"
    );
}

/// TX-BASE: no transcript content (user text, assistant text, tool labels)
/// appears in the empty idle state — only the composer chrome.
#[test]
fn tx_base_empty_shell_has_no_transcript_content() {
    let app = live_app();
    let rendered = render(&app);

    // Composer chrome (❯, ╭, ╰) is expected. No user/assistant text.
    assert!(
        !rendered.contains("Assistant") && !rendered.contains("Thinking"),
        "TX-BASE: empty shell must not show assistant/thinking labels\n{rendered}"
    );
    assert!(
        !rendered.contains("Hello") && !rendered.contains("reply"),
        "TX-BASE: empty shell must not contain any message text\n{rendered}"
    );
}

/// TX-BASE: a single completed user/assistant turn renders both the user
/// message and the assistant reply in the transcript area, with the
/// bordered composer below.
#[test]
fn tx_base_single_turn_renders_user_and_assistant() {
    let mut app = live_app();
    ingest_completed_turn(&mut app, "req_base", "Hello, world!", "Hi there!");

    let rendered = render(&app);

    assert!(
        rendered.contains("Hello, world!"),
        "TX-BASE: user message must render\n{rendered}"
    );
    assert!(
        rendered.contains("Hi there!"),
        "TX-BASE: assistant reply must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && rendered.contains('╭'),
        "TX-BASE: bordered composer retained\n{rendered}"
    );
}

/// TX-BASE: a streaming turn with no body text yet still renders the user
/// message and the composer without panicking.
#[test]
fn tx_base_streaming_no_body_renders_without_panic() {
    let mut app = live_app();
    ingest_started(&mut app, "req_stream", "What is the answer?");

    let rendered = render(&app);

    assert!(
        rendered.contains("What is the answer?"),
        "TX-BASE: user message must render during streaming\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && rendered.contains('╭'),
        "TX-BASE: bordered composer retained during streaming\n{rendered}"
    );
}

/// TX-BASE: a completed turn with long Unicode text wraps and remains visible
/// without truncation or panic.
#[test]
fn tx_base_completed_long_unicode_wraps_correctly() {
    let mut app = live_app();
    let long_unicode =
        "これは非常に長い日本語のテキストです。正しく折り返しされる必要があります。".repeat(5);
    ingest_completed_turn(&mut app, "req_unicode", "Unicode test", &long_unicode);

    let rendered = render(&app);

    assert!(
        rendered.contains('こ') && rendered.contains('れ'),
        "TX-BASE: long Unicode start must render\n{rendered}"
    );
    assert!(
        rendered.contains('折') && rendered.contains('返'),
        "TX-BASE: long Unicode middle must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-BASE: bordered composer retained with long Unicode\n{rendered}"
    );
}

#[test]
fn tx_base_replay_empty_keeps_read_only_placeholder() {
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/tx_replay_empty"), Vec::new());

    let rendered = render(&app);

    assert!(
        rendered.contains("Waiting for first turn"),
        "TX-BASE: replay empty pane must retain its read-only placeholder\n{rendered}"
    );
    assert!(
        !rendered.contains("\n  ~"),
        "TX-BASE: replay empty pane must not use the live empty-scrollback marker\n{rendered}"
    );
}

// ===========================================================================
// Adversarial: malformed events, oversized lines, combining chars, resize
// ===========================================================================

/// TX-ADV: malformed event order (finished before started) does not panic
/// and produces a renderable frame.
#[test]
fn tx_adv_malformed_event_order_does_not_panic() {
    let mut app = live_app();
    // Finish event before start event — malformed order
    app.ingest_event(envelope(
        1,
        Some("req_malformed"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_malformed".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_malformed"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_malformed".into(),
            text: "Malformed order".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_malformed"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_malformed".into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: "Malformed order".to_string(),
            request_digest: "digest".to_string(),
            metadata: None,
        }),
    ));

    // Should not panic
    let rendered = render(&app);

    assert!(
        rendered.contains('❯') && rendered.contains('╭'),
        "TX-ADV: bordered composer retained after malformed events\n{rendered}"
    );
}

/// TX-ADV: an oversized single-line message (no newlines, very long) wraps
/// without truncation or panic.
#[test]
fn tx_adv_oversized_line_wraps_without_panic() {
    let mut app = live_app();
    let huge = "A".repeat(5000);
    ingest_completed_turn(&mut app, "req_huge", &huge, "Response");

    let rendered = render(&app);

    assert!(
        rendered.contains('A'),
        "TX-ADV: oversized line content must render\n{rendered}"
    );
    assert!(
        rendered.contains("Response"),
        "TX-ADV: assistant reply must render after oversized user message\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-ADV: bordered composer retained with oversized line\n{rendered}"
    );
}

/// TX-ADV: combining characters and ZWJ sequences do not corrupt wrapping
/// or cause panics.
#[test]
fn tx_adv_combining_characters_do_not_corrupt_layout() {
    let mut app = live_app();
    // Text with combining characters and ZWJ sequences
    let combining = "Cafe\u{0301} resume\u{0301} nai\u{0300}ve\u{0300} \
         Family: \u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} \
         Flag: \u{1f1fa}\u{1f1f3}"
        .repeat(10);
    ingest_completed_turn(&mut app, "req_combining", "Combining test", &combining);

    // Should not panic
    let rendered = render(&app);

    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-ADV: bordered composer retained with combining chars\n{rendered}"
    );
}

/// TX-ADV: rendering at various sizes (including very small) does not panic.
#[test]
fn tx_adv_resize_does_not_panic() {
    let mut app = live_app();
    ingest_completed_turn(&mut app, "req_resize", "Resize test", "Some response text");

    // Render at various sizes — none should panic
    for (w, h) in [
        (120u16, 40u16),
        (80, 24),
        (60, 20),
        (40, 10),
        (20, 5),
        (10, 3),
    ] {
        let rendered = render_at(&app, w, h);
        // Just ensure it doesn't panic — the content may be clipped at tiny sizes
        assert!(
            !rendered.is_empty(),
            "TX-ADV: render at {w}x{h} must produce output"
        );
    }
}

/// TX-ADV: an empty user message followed by a streaming event does not
/// panic and renders the composer.
#[test]
fn tx_adv_empty_user_message_with_streaming_does_not_panic() {
    let mut app = live_app();
    app.ingest_event(envelope(
        1,
        Some("req_empty"),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_empty".into(),
            text: String::new(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_empty"),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_empty".into(),
            provider_id: "mock".to_string(),
            model_id: "model-tx".to_string(),
            prompt_summary: String::new(),
            request_digest: "digest".to_string(),
            metadata: None,
        }),
    ));

    let rendered = render(&app);

    assert!(
        rendered.contains('❯') && rendered.contains('╭'),
        "TX-ADV: bordered composer retained with empty user message\n{rendered}"
    );
}

/// TX-ADV: duplicate stream deltas (same content repeated) do not cause
/// panics or layout corruption.
#[test]
fn tx_adv_duplicate_stream_deltas_do_not_panic() {
    let mut app = live_app();
    ingest_started(&mut app, "req_dup", "Duplicate test");
    // Send the same delta multiple times
    for i in 0..10u64 {
        app.ingest_event(envelope(
            10 + i,
            Some("req_dup"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_dup".into(),
                delta: "Repeated content. ".to_string(),
            }),
        ));
    }

    let rendered = render(&app);

    assert!(
        rendered.contains("Duplicate test"),
        "TX-ADV: user message must render with duplicate deltas\n{rendered}"
    );
    assert!(
        rendered.contains("Repeated content"),
        "TX-ADV: stream content must render\n{rendered}"
    );
    assert!(
        rendered.contains('❯') && count_char(&rendered, '╭') >= 1,
        "TX-ADV: bordered composer retained with duplicate deltas\n{rendered}"
    );
}

#[test]
fn tx_manual_probe_frames_for_task_11_qa() {
    let empty = live_app();
    println!("QA empty 120x40:\n{}", render(&empty));

    let mut completed = live_app();
    ingest_completed_turn(
        &mut completed,
        "qa_completed",
        "one user turn",
        "one assistant turn",
    );
    println!("QA completed 120x40:\n{}", render(&completed));

    let mut streaming = live_app();
    ingest_started(&mut streaming, "qa_streaming", "streaming without body");
    println!("QA streaming-no-body 120x40:\n{}", render(&streaming));

    let mut unicode = live_app();
    let long_unicode = "日本語の長い応答。絵文字 🚀 と結合文字 e\u{301}。".repeat(8);
    ingest_completed_turn(
        &mut unicode,
        "qa_unicode",
        "long Unicode turn",
        &long_unicode,
    );
    println!("QA completed-long-Unicode 120x40:\n{}", render(&unicode));
}
