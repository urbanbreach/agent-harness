use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::{
    app::AppState,
    theme::{ColorLevel, GlyphMode},
    ui::render_app,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};
use serde_json::json;
use std::{fs, path::Path, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn new_app(session: Option<&Path>) -> AppState {
    let mut app = AppState::new_live(session.map(Path::to_path_buf), false, None);
    app.set_startup_logo_capabilities_for_evidence(ColorLevel::TrueColor, GlyphMode::Preferred);
    app.restart_motion_epoch_for_evidence();
    app.advance_wall_clock_for_motion_evidence(Duration::ZERO);
    app.set_generic_tool_output_visible_for_test(true);
    app
}

fn screen(app: &AppState, width: u16) -> Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, 44))?;
    terminal.draw(|frame| render_app(frame, app))?;
    Ok(terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n"))
}

#[test]
fn edit_diff_keeps_lsp_errors_and_unavailable_checks_visible() -> Result<()> {
    for tool in ["write", "edit", "apply_patch"] {
        for diagnostic in [
            "LSP errors detected in this file, please fix:\nsrc/lib.rs:1:1 Error broken source",
            "LSP diagnostics unavailable: language server failed",
        ] {
            let session = tempfile::tempdir()?;
            fs::create_dir(session.path().join("artifacts"))?;
            fs::write(
                session.path().join("artifacts/edit.diff"),
                "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+pub fn broken() {}\n",
            )?;
            let mut app = new_app(Some(session.path()));
            let args = match tool {
                "write" => json!({"path":"src/lib.rs","content":"pub fn broken() {}"}),
                "edit" => json!({"path":"src/lib.rs","oldString":"old","newString":"new"}),
                _ => {
                    json!({"patchText":"*** Begin Patch\n*** Add File: src/lib.rs\n+pub fn broken() {}\n*** End Patch"})
                }
            };
            let payloads = [
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "turn".into(),
                    text: "Verify the edit".into(),
                }),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "turn".into(),
                    provider_id: "fixture".into(),
                    model_id: "fixture".into(),
                    prompt_summary: "Verify the edit".into(),
                    request_digest: "fixture".into(),
                    metadata: None,
                }),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "call".into(),
                    tool_id: tool.into(),
                    args_summary: args.to_string(),
                    args_digest: "fixture".into(),
                    metadata: None,
                }),
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "call".into(),
                }),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "call".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some(format!("Edit saved.\n\n{diagnostic}")),
                    output_digest: None,
                    output_json: Some(
                        json!({"edits":[{"path":"src/lib.rs","diff_rel_path":"artifacts/edit.diff"}]}),
                    ),
                    metadata: None,
                }),
            ];
            for (index, payload) in payloads.into_iter().enumerate() {
                if matches!(&payload, EventV1::ToolCallFinished(_)) {
                    screen(&app, 80)?;
                }
                app.ingest_event(EventEnvelopeV1 {
                    schema_version: SCHEMA_VERSION,
                    event_id: format!("event-{index}"),
                    seq: index as u64 + 1,
                    run_id: "lsp-capture".into(),
                    mono_ms: index as u64,
                    ts: None,
                    actor: EventActor::new(ActorKind::Worker, Some("worker".into())),
                    correlation_id: Some("turn".into()),
                    causation_id: None,
                    stream_key: None,
                    payload,
                });
            }
            app.expand_all_tool_outputs_for_test();
            app.advance_wall_clock_for_motion_evidence(Duration::from_secs(60));
            app.refresh_motion_for_evidence();
            let text = screen(&app, 80)?;
            assert!(
                text.contains("pub fn broken"),
                "{tool} did not render the diff:\n{text}"
            );
            assert!(
                text.contains("LSP"),
                "{tool} hid diagnostics behind its diff:\n{text}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires recorded coordinator LSP evidence; run scripts/qa/verify-lsp.sh"]
fn capture_recorded_lsp_tool_results_through_production_renderer() -> Result<()> {
    let root = std::path::PathBuf::from(std::env::var("HARNESS_LSP_EVIDENCE_DIR")?);
    let out = root.join("ansi");
    fs::create_dir_all(&out)?;
    let mut captures = 0;
    for source in ["simulation", "native"] {
        let session = root.join(source);
        let events = fs::read_to_string(session.join("events.jsonl"))?
            .lines()
            .map(serde_json::from_str::<EventEnvelopeV1>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for width in [40, 80, 120] {
            let mut app = new_app(Some(&session));
            for event in &events {
                if matches!(&event.payload, EventV1::ToolCallFinished(_)) {
                    screen(&app, width)?;
                }
                app.ingest_event(event.clone());
                assert_eq!(app.canonical_projection_error(), None);
                let EventV1::ToolCallFinished(finished) = &event.payload else {
                    continue;
                };
                app.expand_all_tool_outputs_for_test();
                app.advance_wall_clock_for_motion_evidence(Duration::from_secs(60 + event.seq));
                app.refresh_motion_for_evidence();
                let text = screen(&app, width)?;
                let summary = finished
                    .output_summary
                    .as_deref()
                    .ok_or("missing tool result")?;
                let expected = if summary.contains("LSP diagnostics unavailable") {
                    "LSP diagnostics unavailable"
                } else if summary.contains("no issues found") {
                    "no issues found"
                } else if summary.contains("No diagnostics found") {
                    "No diagnostics found"
                } else if finished.status == ToolCallStatus::Failed {
                    "failed to start language server"
                } else {
                    "Error"
                };
                let compact = |value: &str| {
                    value
                        .chars()
                        .filter(char::is_ascii_alphanumeric)
                        .collect::<String>()
                };
                assert!(
                    compact(&text).contains(&compact(expected)),
                    "{source}/{} at {width} columns hid {expected}:\n{text}",
                    event.seq
                );
                let name = format!("lsp-{source}-{}-{width}x44-motion-0ms", event.seq);
                let mut bytes = Vec::new();
                {
                    let mut terminal = Terminal::with_options(
                        CrosstermBackend::new(&mut bytes),
                        TerminalOptions {
                            viewport: Viewport::Fixed(Rect::new(0, 0, width, 44)),
                        },
                    )?;
                    terminal.draw(|frame| render_app(frame, &app))?;
                }
                fs::write(out.join(format!("{name}.ansi")), bytes)?;
                fs::write(out.join(format!("{name}.txt")), text)?;
                fs::write(
                    out.join(format!("{name}.result.json")),
                    serde_json::to_vec_pretty(finished)?,
                )?;
                captures += 1;
            }
        }
    }
    assert_eq!(
        captures, 39,
        "seven simulated and six native results at three widths"
    );
    fs::write(
        out.join("producer.json"),
        serde_json::to_vec_pretty(&json!({
            "entrypoints":["CoordinatorHandle::execute_agent_tool_call", "AppState::ingest_event", "harness_tui::ui::render_app"],
            "timing":{"mode":"replay of actual coordinator events; settled motion clock"},
            "sources":{"native":"installed rust-analyzer", "simulation":"scripted local LSP peer"},
            "captures":captures,
        }))?,
    )?;
    Ok(())
}
