//! Offline runtime fixture for `scripts/measure-tui-runtime.py`; never calls a provider.
use std::time::Duration;

use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions,
};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scenario = std::env::args().nth(1).unwrap_or_else(|| "idle".into());
    let (sender, update_rx) = live_update_channel();
    if scenario == "eval" {
        let mut seq = 0;
        let mut send = |kind, data| -> Result<(), Box<dyn std::error::Error>> {
            seq += 1;
            sender.send(LiveUpdate::Event(Box::new(serde_json::from_value(
                json!({
                    "delivery": "durable",
                    "event": {
                        "schema_version": 1, "event_id": format!("probe-{seq}"), "seq": seq,
                        "run_id": "probe", "mono_ms": seq, "ts": null,
                        "actor": {"kind": "worker", "agent_id": "worker"},
                        "correlation_id": "turn", "causation_id": null, "stream_key": null,
                        "payload": {"event_type": kind, "data": data}
                    }
                }),
            )?)))?;
            Ok(())
        };
        send(
            "user_message_submitted",
            json!({"request_id":"turn", "text":"Check the workspace"}),
        )?;
        send(
            "provider_request_started",
            json!({"request_id":"turn", "provider_id":"mock", "model_id":"mock", "prompt_summary":"Fixture", "request_digest":"fixture", "metadata":null}),
        )?;
        for id in 0..3 {
            let tool_call_id = format!("eval-{id}");
            send(
                "tool_call_requested",
                json!({"tool_call_id":tool_call_id, "tool_id":"eval",
                "args_summary":json!({"language":"js", "summary":"Check workspace", "code":"const result = await tools.shell.run({command: 'pwd'});\nconsole.log(result);"}).to_string(), "args_digest":"fixture"}),
            )?;
            send(
                "tool_call_started",
                json!({"tool_call_id":tool_call_id, "tool_id":"eval", "metadata":null}),
            )?;
        }
    }
    let mode = if matches!(scenario.as_str(), "idle" | "handoff" | "handoff_failure") {
        TuiMode::Replay {
            run_dir: std::env::current_dir()?,
            events: Vec::new(),
        }
    } else if scenario == "startup" {
        TuiMode::Startup {
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
        }
    } else {
        TuiMode::Live {
            run_dir: std::env::current_dir()?,
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        }
    };
    let worker = (scenario == "burst").then(|| {
        let sender = sender.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            for index in 0..10_000 {
                if sender
                    .send(LiveUpdate::OperatorNotice {
                        message: format!("Synthetic update {index}"),
                        level: OperatorNoticeLevel::Info,
                    })
                    .is_err()
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    });
    run_tui_with_options(TuiOptions {
        mode,
        exit_on_finish: false,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: matches!(scenario.as_str(), "handoff" | "handoff_failure"),
        skip_alternate_screen: false,
    })?;
    let handoffs = match scenario.as_str() {
        "handoff" => 3,
        "handoff_failure" => 1,
        _ => 0,
    };
    for index in 0..handoffs {
        if scenario == "handoff_failure" {
            let trace = std::path::PathBuf::from(
                std::env::var_os("HARNESS_TUI_PRESENTATION_TRACE")
                    .ok_or("missing injected trace path")?,
            );
            let parent = trace.parent().ok_or("trace path has no parent")?;
            std::fs::rename(parent, parent.with_extension("completed"))?;
            std::fs::write(parent, "fixture\n")?;
        }
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Replay {
                run_dir: std::env::current_dir()?,
                events: Vec::new(),
            },
            exit_on_finish: false,
            on_ui_intent: None,
            keybindings: None,
            toggles: None,
            preserve_terminal_on_exit: index + 1 < handoffs,
            skip_alternate_screen: false,
        })?;
    }
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    drop(sender);
    Ok(())
}
