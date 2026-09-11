//! Exact-clock production-renderer evidence, independent of runtime PTY timing.
use std::{fs, path::Path, time::Duration};

use harness_tui::{
    app::AppState,
    theme::{ColorLevel, GlyphMode},
    theme_family::{serialize_choice, ThemeChoice},
    ui::render_app,
    UnwrapOrAbort,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};

const SIZES: [(u16, u16); 8] = [
    (80, 24),
    (120, 40),
    (160, 50),
    (89, 32),
    (90, 32),
    (90, 24),
    (120, 32),
    (200, 60),
];
const TIMES: [u64; 5] = [0, 100, 300, 1300, 4000];

#[path = "support/grok_alignment_recorded.rs"]
mod grok_alignment;

#[test]
fn welcome_geometry_and_animation_use_immediate_controls_and_exact_clock() {
    for (profile, color, glyphs, choice) in [
        (
            "dark",
            ColorLevel::TrueColor,
            GlyphMode::Preferred,
            ThemeChoice::Dark,
        ),
        (
            "light",
            ColorLevel::TrueColor,
            GlyphMode::Preferred,
            ThemeChoice::Light,
        ),
        (
            "basic",
            ColorLevel::Basic,
            GlyphMode::Preferred,
            ThemeChoice::Dark,
        ),
        (
            "ascii",
            ColorLevel::None,
            GlyphMode::Ascii,
            ThemeChoice::Dark,
        ),
    ] {
        for (width, height) in SIZES {
            for reduced in [false, true] {
                verify_welcome_case(profile, color, glyphs, choice, width, height, reduced);
            }
        }
    }
}

fn verify_welcome_case(
    profile: &str,
    color: ColorLevel,
    glyphs: GlyphMode,
    choice: ThemeChoice,
    width: u16,
    height: u16,
    reduced: bool,
) {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.restore_theme_choice(&serialize_choice(choice).unwrap_or_abort())
        .unwrap_or_abort();
    app.set_startup_logo_capabilities_for_evidence(color, glyphs);
    app.set_reduced_motion_for_evidence(reduced);
    app.restart_motion_epoch_for_evidence();
    let first = render(&app, width, height);
    let first_text = text(&first);
    let mut changed = false;
    let mut previous_time = 0;
    for milliseconds in TIMES {
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(
            milliseconds - previous_time,
        ));
        previous_time = milliseconds;
        let frame = render(&app, width, height);
        let text = text(&frame);
        assert_eq!(
            text, first_text,
            "welcome content changed at {width}x{height}, t={milliseconds}"
        );
        for control in ["New worktree", "Resume session", "Changelog", "Quit"] {
            assert!(
                text.contains(control),
                "missing {control} at {width}x{height}: {text}"
            );
        }
        assert_eq!(
            text.matches(&format!("Harness {}", env!("CARGO_PKG_VERSION")))
                .count(),
            1,
            "identity duplication: {text}"
        );
        changed |= frame != first;
        if reduced {
            assert_eq!(frame, first, "reduced motion changed cells");
        }
        persist_if_requested(&app, width, height, milliseconds, reduced, profile);
    }
    if !reduced && color == ColorLevel::TrueColor && text(&first).contains('█') {
        assert!(changed, "visible H must shimmer at {width}x{height}");
    }
}

fn render(app: &AppState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn persist_if_requested(
    app: &AppState,
    width: u16,
    height: u16,
    milliseconds: u64,
    reduced: bool,
    profile: &str,
) {
    let name = format!(
        "welcome-{profile}-{width}x{height}-{}-{milliseconds}ms",
        if reduced { "reduced" } else { "motion" }
    );
    persist_frame(app, width, height, &name);
}

fn persist_frame(app: &AppState, width: u16, height: u16, name: &str) {
    let Some(directory) = std::env::var_os("HARNESS_PARITY_RENDER_ARTIFACT_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    fs::create_dir_all(directory).unwrap_or_abort();
    let mut bytes = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut bytes);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
            },
        )
        .unwrap_or_abort();
        terminal
            .draw(|frame| render_app(frame, app))
            .unwrap_or_abort();
    }
    fs::write(directory.join(format!("{name}.ansi")), bytes).unwrap_or_abort();
    fs::write(
        directory.join(format!("{name}.txt")),
        text(&render(app, width, height)),
    )
    .unwrap_or_abort();
}

// Synthetic event data only: no provider or filesystem work is needed for these frames.
#[test]
fn chat_and_tool_bullets_animate_without_recoloring_labels_or_reflowing_text() {
    for (width, height) in [(40, 24), (80, 24), (120, 40)] {
        for reduced in [false, true] {
            for scene in [
                "thinking",
                "running",
                "runningopen",
                "success",
                "successopen",
                "failed",
                "failedopen",
                "thinkingcode",
                "context",
                "contextopen",
                "searchsources",
                "commands",
                "commandsopen",
                "answer",
            ] {
                verify_chat_scene(width, height, reduced, scene);
            }
        }
    }
}

fn chat_app(scene: &str, reduced: bool) -> AppState {
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderReasoningDeltaEvent,
        ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
        SCHEMA_VERSION,
    };
    let mut app = AppState::new_live(None, false, None);
    app.restart_motion_epoch_for_evidence();
    app.set_reduced_motion_for_evidence(reduced);
    let mut seq = 0;
    let mut ingest = |payload| {
        seq += 1;
        app.ingest_event(EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("chat-parity-{seq}"),
            seq,
            run_id: "chat-parity".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, None),
            correlation_id: Some("chat-parity-request".into()),
            causation_id: None,
            stream_key: None,
            payload,
        });
    };
    ingest(EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
        request_id: "chat-parity-request".into(),
        text: "Inspect the renderer".into(),
    }));
    ingest(EventV1::ProviderRequestStarted(
        ProviderRequestStartedEvent {
            request_id: "chat-parity-request".into(),
            provider_id: "mock".into(),
            model_id: "model".into(),
            prompt_summary: "Inspect the renderer".into(),
            request_digest: "synthetic".into(),
            metadata: None,
        },
    ));
    if scene.starts_with("thinking") {
        ingest(EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "chat-parity-request".into(),
            delta: if scene == "thinkingcode" { "```rust\nlet text = r#\"\ninside the string\n\"#;" } else { "Check **terminal geometry**, then compare the tool output.\n\nKeep the title stationary while the diamond and rail animate." }.into(),
        }));
    } else if scene == "answer" {
        ingest(EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "chat-parity-request".into(),
            delta: "The **renderer** preserves Unicode: 界 e\u{301} 👩\u{200d}💻.\n\n```rust\nfn main() {\n    println!(\"ready\");\n}\n```\n\n- Stable text\n- Safe output".into(),
        }));
    } else if scene.starts_with("context")
        || scene == "searchsources"
        || scene.starts_with("commands")
    {
        if scene.starts_with("context") {
            ingest(EventV1::ProviderReasoningDelta(
                ProviderReasoningDeltaEvent {
                    request_id: "chat-parity-request".into(),
                    delta: "Inspect the files first.".into(),
                },
            ));
        }
        let count = if scene.starts_with("commands") { 14 } else { 2 };
        for index in 0..count {
            let tool_id = if scene.starts_with("commands") {
                "bash"
            } else if scene == "searchsources" {
                "search.web"
            } else {
                "fs.read"
            };
            let args = if tool_id == "bash" {
                serde_json::json!({"command": format!("printf command-{index:02}")})
            } else if tool_id == "search.web" {
                serde_json::json!({"query": format!("query {index}")})
            } else {
                serde_json::json!({"path": format!("src/file-{index}.rs")})
            };
            ingest(EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: format!("chat-group-{index}").into(),
                tool_id: tool_id.into(),
                args_summary: args.to_string(),
                args_digest: "synthetic".into(),
                metadata: None,
            }));
            ingest(EventV1::ToolCallFinished(ToolCallFinishedEvent { tool_call_id: format!("chat-group-{index}").into(), status: ToolCallStatus::Succeeded, output_summary: Some("recorded output".into()), output_digest: None, output_json: (scene == "searchsources").then(|| serde_json::json!({"sources": ["https://example.com/shared", format!("https://example.com/{index}")]})), metadata: None }));
        }
        if scene.starts_with("context") {
            ingest(EventV1::ProviderReasoningDelta(
                ProviderReasoningDeltaEvent {
                    request_id: "chat-parity-request".into(),
                    delta: "Still checking **sources**.".into(),
                },
            ));
        }
    } else {
        ingest(EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "chat-parity-tool".into(),
            tool_id: "bash".into(),
            args_summary:
                r#"{"command":"printf 'ready\\n'","description":"Check terminal output"}"#.into(),
            args_digest: "synthetic".into(),
            metadata: None,
        }));
        ingest(EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "chat-parity-tool".into(),
        }));
        if !scene.starts_with("running") {
            ingest(EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "chat-parity-tool".into(),
                status: if scene.starts_with("failed") {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Succeeded
                },
                output_summary: Some(
                    if scene.starts_with("failed") {
                        "terminal unavailable"
                    } else {
                        "ready"
                    }
                    .into(),
                ),
                output_digest: None,
                output_json: None,
                metadata: None,
            }));
        }
    }
    if scene == "contextopen" || scene == "commandsopen" {
        app.focus = harness_tui::app::Focus::Details;
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyModifiers::NONE,
        ));
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        app.focus = harness_tui::app::Focus::Prompt;
    } else if scene.ends_with("open") {
        app.toggle_tool_output_for_test("chat-parity-tool");
        assert!(app.is_tool_output_expanded_for_test("chat-parity-tool"));
    }
    if scene.starts_with("commands") {
        app.set_transcript_scroll_for_test(usize::MAX);
    }
    app
}

fn chat_scene_label(scene: &str) -> &'static str {
    match scene {
        "thinking" | "thinkingcode" | "context" | "contextopen" => "Thinking…",
        "searchsources" => "Searched 3 websites",
        "commandsopen" => "Ran 14 commands",
        "commands" => "Ran 4 commands",
        _ => "Run Check",
    }
}

fn verify_chat_scene(width: u16, height: u16, reduced: bool, scene: &str) {
    let mut app = chat_app(scene, reduced);
    let transcript =
        harness_tui::layout::FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height))
            .transcript
            .unwrap_or_abort();
    let chat_frame = |app: &AppState| {
        let mut buffer = render(app, width, height);
        for y in 0..height {
            if y < transcript.y || y >= transcript.bottom() {
                for x in 0..width {
                    buffer[(x, y)].reset();
                }
            }
        }
        buffer
    };
    let first = chat_frame(&app);
    let label = chat_scene_label(scene);
    let label_cells = |buffer: &Buffer| {
        buffer.content.chunks(usize::from(width)).find_map(|row| {
            row.windows(label.chars().count())
                .find(|cells| {
                    cells
                        .iter()
                        .zip(label.chars())
                        .all(|(cell, ch)| cell.symbol() == ch.to_string())
                })
                .map(<[_]>::to_vec)
        })
    };
    let mut previous = 0;
    for milliseconds in [0, 330, 660] {
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(milliseconds - previous));
        previous = milliseconds;
        let frame = chat_frame(&app);
        assert_eq!(
            text(&frame),
            text(&first),
            "{scene}: animation reflowed text"
        );
        if scene != "answer" {
            assert!(
                label_cells(&frame).is_some(),
                "missing {label}: {}",
                text(&frame)
            );
            assert_eq!(
                label_cells(&frame),
                label_cells(&first),
                "{scene}: title animated"
            );
            assert!(text(&frame).contains('◆') || text(&frame).contains('◈'));
            if scene == "running" {
                assert!(!text(&frame).contains('┃'), "collapsed tool painted a rail");
            } else if matches!(scene, "runningopen" | "successopen" | "failedopen") {
                assert!(text(&frame).contains('┃'), "open command lost its rail");
            }
        }
        if reduced {
            assert_eq!(frame, first, "{scene}: reduced motion changed cells");
        }
        if milliseconds == 330
            && !reduced
            && matches!(scene, "thinking" | "running" | "runningopen")
        {
            assert_ne!(frame, first, "{scene}: running indicator did not animate");
        }
        persist_frame(
            &app,
            width,
            height,
            &format!(
                "chat-{scene}-{width}x{height}-{}-{milliseconds}ms",
                if reduced { "reduced" } else { "motion" }
            ),
        );
    }
}
