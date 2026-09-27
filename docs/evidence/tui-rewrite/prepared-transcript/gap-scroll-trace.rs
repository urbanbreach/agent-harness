use harness_core::event::EventEnvelopeV1;
use harness_tui::ui::render_app;
use ratatui::{backend::TestBackend, layout::Rect, Terminal};
use serde_json::json;
use std::{fs, path::PathBuf};
#[path = "support/rewrite_journey.rs"]
mod journey;
use journey::{envelope, Journey, Result};
fn history(turns: usize) -> Result<Vec<EventEnvelopeV1>> {
    let mut result = Vec::with_capacity(turns * 4);
    for index in 0..turns {
        let request = format!("history-{index}");
        for (kind, data) in [
            (
                "user_message_submitted",
                json!({"request_id": request, "text": format!("Prompt {index:05}")}),
            ),
            (
                "provider_request_started",
                json!({"request_id": request, "provider_id": "mock",
                "model_id": "reference", "prompt_summary": "Fixture", "request_digest": "fixture", "metadata": null}),
            ),
            (
                "assistant_message_finished",
                json!({"request_id": request, "tool_call_count": 0,
                "parts": [{"kind": "text", "text": format!("Response {index:05}: 界 e\u{301} 👩‍💻 retained history.")}],
                "provenance": null, "assistant_message": null}),
            ),
            (
                "provider_request_finished",
                json!({"request_id": request, "finish_reason": "stop",
                "output_digest": "fixture", "usage": null, "metadata": null}),
            ),
        ] {
            let seq = u64::try_from(result.len())? + 1;
            let mut value = envelope(seq, kind, data);
            value["correlation_id"] = json!(request);
            result.push(serde_json::from_value(value)?);
        }
    }
    Ok(result)
}

#[test]
fn recorded_single_row_scroll_trace() -> Result {
    let area = Rect::new(0, 0, 160, 48);
    let mut reports = Vec::new();
    let workspace = tempfile::tempdir()?;
    std::env::set_current_dir(workspace.path())?;
    for settle in [false, true] {
        let mut j = Journey::new(false);
        j.app.replace_events(history(1000)?);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height))?;
        j.app.set_frame_area(area);
        terminal.draw(|f| render_app(f, &j.app))?;
        j.app.scroll_page_up(24);
        let mut frames = Vec::new();
        for index in 0..80 {
            if index % 40 < 20 {
                j.app.scroll_page_up(1);
            } else {
                j.app.scroll_page_down(1);
            }
            j.app.set_frame_area(area);
            let before = j.app.transcript_scroll_offset();
            terminal.draw(|f| render_app(f, &j.app))?;
            if settle {
                j.app.set_frame_area(area);
                terminal.draw(|f| render_app(f, &j.app))?;
            }
            let text = terminal
                .backend()
                .buffer()
                .content
                .chunks(usize::from(area.width))
                .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join(
                    "
",
                );
            frames.push(json!({"index":index,"before":before,"after":j.app.transcript_scroll_offset(),"text":text}));
        }
        reports.push(json!({"settle":settle,"frames":frames}));
    }
    fs::write(
        PathBuf::from(std::env::var_os("HARNESS_REWRITE_TRACE_OUT").ok_or("missing output")?),
        serde_json::to_vec(&reports)?,
    )?;
    Ok(())
}
