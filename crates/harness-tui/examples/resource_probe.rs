//! Offline runtime fixture for `scripts/measure-tui-runtime.py`; never calls a provider.
use std::time::Duration;

use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, OperatorNoticeLevel, TuiMode, TuiOptions,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scenario = std::env::args().nth(1).unwrap_or_else(|| "idle".into());
    let (sender, update_rx) = live_update_channel();
    let mode = if scenario == "idle" {
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
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })?;
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    drop(sender);
    Ok(())
}
