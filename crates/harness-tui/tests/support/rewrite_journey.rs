//! Synthetic inputs shared by reference and replacement. No private TUI state.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{EventEnvelopeV1, RuntimeEvent};
use harness_tui::app::{AppState, LaunchMetadata, ModelOption, UiIntent};
use harness_tui::theme::{ColorLevel, GlyphMode};
use serde_json::{json, Value};

pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Journey {
    pub app: AppState,
    pub inputs: Vec<Value>,
    pub intents: Arc<Mutex<Vec<String>>>,
    pub seq: u64,
}

impl Journey {
    pub fn new(startup: bool) -> Self {
        harness_tui::app::set_pending_live_prompt_draft(None);
        let intents = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&intents);
        let callback = Arc::new(move |intent: UiIntent| {
            recorded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(format!("{intent:?}"));
        });
        let mut app = if startup {
            AppState::new_startup(Vec::new(), Some(callback))
        } else {
            AppState::new_live(None, false, Some(callback))
        };
        app.set_launch_metadata(
            LaunchMetadata::from_model_ref("worker", "mock:reference")
                .with_mode_label("Demo")
                .with_available_models(vec![
                    ModelOption::from_model_ref("worker", "mock:reference"),
                    ModelOption::from_model_ref("reviewer", "mock:review"),
                ]),
        );
        app.set_startup_logo_capabilities_for_evidence(ColorLevel::TrueColor, GlyphMode::Preferred);
        // Match the fixed workspace label used by the original evidence mode.
        // Workspace-dependent dialogs in this journey exercise unavailable states.
        app.set_file_mention_workspace_root_for_test("/workspace/agent-harness".into());
        app.set_reduced_motion_for_evidence(true);
        app.restart_motion_epoch_for_evidence();
        Self {
            app,
            inputs: Vec::new(),
            intents,
            seq: 0,
        }
    }

    pub fn text(&mut self, text: &str) {
        self.inputs.push(json!({"type": text}));
        for ch in text.chars() {
            self.app
                .handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    }

    pub fn key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.inputs
            .push(json!({"key": format!("{code:?}"), "modifiers": modifiers.bits()}));
        self.app.handle_key(KeyEvent::new(code, modifiers));
    }

    pub fn paste(&mut self, text: &str) {
        self.inputs.push(json!({"paste": text}));
        self.app.handle_paste(text);
    }

    pub fn tick(&mut self, milliseconds: u64) {
        self.inputs.push(json!({"advance_ms": milliseconds}));
        self.app
            .advance_wall_clock_for_motion_evidence(Duration::from_millis(milliseconds));
        self.app.refresh_motion_for_evidence();
    }

    pub fn event(&mut self, event_type: &str, data: Value) -> Result {
        self.seq += 1;
        let envelope = envelope(self.seq, event_type, data);
        self.inputs.push(json!({"durable": envelope}));
        self.app
            .ingest_event(serde_json::from_value::<EventEnvelopeV1>(envelope)?);
        Ok(())
    }

    pub fn live(&mut self, event_type: &str, data: Value) -> Result {
        let event = json!({"delivery": "live", "event": envelope(self.seq, event_type, data)});
        self.inputs.push(json!({"runtime": event}));
        self.app
            .ingest_runtime_event(serde_json::from_value::<RuntimeEvent>(event)?);
        Ok(())
    }

    pub fn start(&mut self, request: &str, prompt: &str) -> Result {
        self.event(
            "user_message_submitted",
            json!({"request_id": request, "text": prompt}),
        )?;
        self.event(
            "provider_request_started",
            json!({
                "request_id": request, "provider_id": "mock", "model_id": "reference",
                "prompt_summary": prompt, "request_digest": "fixture", "metadata": null,
            }),
        )
    }

    pub fn finish(&mut self, request: &str, text: &str) -> Result {
        self.event(
            "assistant_message_finished",
            json!({
                "request_id": request, "tool_call_count": 0,
                "parts": [{"kind": "text", "text": text}],
                "provenance": null, "assistant_message": null,
            }),
        )?;
        self.event(
            "provider_request_finished",
            json!({
                "request_id": request, "finish_reason": "stop", "output_digest": "fixture",
                "usage": null, "metadata": null,
            }),
        )
    }

    pub fn permission(&mut self, question: bool) -> Result {
        self.event(
            "permission_requested",
            json!({
                "permission_id": "permission", "tool_call_id": "tool",
                "kind": if question { "question" } else { "bash" },
                "summary": if question { json!({"questions": [{
                    "custom": true,
                    "header": "Storage",
                    "options": [{"description": "One local file", "label": "SQLite"},
                                {"description": "Discard on exit", "label": "Memory"}],
                    "question": "Which local storage should the fixture use?",
                }]}).to_string() } else { "Inspect the local fixture\nprintf ready".into() },
                "request_digest": "fixture", "timeout_ms": 30000, "default_decision": "deny",
            }),
        )
    }
}

pub fn envelope(seq: u64, event_type: &str, data: Value) -> Value {
    let correlation_id = data
        .get("request_id")
        .cloned()
        .unwrap_or_else(|| json!("turn"));
    json!({
        "schema_version": 1, "event_id": format!("reference-{seq}"), "seq": seq,
        "run_id": "reference", "mono_ms": seq, "ts": null,
        "actor": {"kind": "worker", "agent_id": "worker"},
        "correlation_id": correlation_id, "causation_id": null, "stream_key": null,
        "payload": {"event_type": event_type, "data": data},
    })
}
