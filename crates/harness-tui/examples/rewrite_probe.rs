//! Offline PTY fixture. A local socket supplies public runtime events; stdin stays the terminal.
#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use harness_core::event::RuntimeEvent;
    use harness_tui::{
        live_update_channel, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
    };
    use serde_json::{json, Value};
    use std::{
        io::{BufRead, Write},
        net::Shutdown,
        os::unix::net::{UnixListener, UnixStream},
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };

    let socket = PathBuf::from(std::env::args().nth(1).ok_or("expected socket path")?);
    let listener = UnixListener::bind(&socket)?;
    let (sender, update_rx) = live_update_channel();
    let stopped = Arc::new(AtomicBool::new(false));
    let connection = Arc::new(Mutex::new(None::<UnixStream>));
    let worker_stopped = Arc::clone(&stopped);
    let worker_connection = Arc::clone(&connection);
    let worker = std::thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let (stream, _) = listener.accept()?;
            *worker_connection
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(stream.try_clone()?);
            if worker_stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            for line in std::io::BufReader::new(stream).lines() {
                let value: Value = serde_json::from_str(&line?)?;
                let update = if let Some(response) = value.get("rewind_points") {
                    LiveUpdate::RewindPoints {
                        generation: serde_json::from_value(response["generation"].clone())?,
                        result: serde_json::from_value(response["result"].clone())?,
                    }
                } else if let Some(response) = value.get("rewind_complete") {
                    LiveUpdate::RewindComplete {
                        generation: serde_json::from_value(response["generation"].clone())?,
                        result: serde_json::from_value(response["result"].clone())?,
                    }
                } else {
                    LiveUpdate::Event(Box::new(serde_json::from_value::<RuntimeEvent>(value)?))
                };
                if sender.send(update).is_err() {
                    break;
                }
            }
            Ok(())
        },
    );
    let intent_connection = Arc::clone(&connection);
    let result = run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: std::env::current_dir()?,
            historical_events: Vec::new(),
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent: Some(Arc::new(move |intent| {
            let value = match intent {
                UiIntent::LoadRewindPoints { generation, .. } => {
                    json!({"load_rewind_points": generation})
                }
                UiIntent::RewindConversation {
                    generation,
                    request_id,
                } => json!({"rewind_conversation": generation, "request_id": request_id}),
                _ => return,
            };
            if let Some(stream) = intent_connection
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_mut()
            {
                // The controller owns this fixture socket; a disconnected controller ends the journey.
                let _ = writeln!(stream, "{value}");
            }
        })),
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    });
    stopped.store(true, Ordering::Release);
    if let Some(stream) = connection
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
    {
        let _ = stream.shutdown(Shutdown::Both);
    }
    // Unblock accept when the TUI exits before the controller connects.
    let _ = UnixStream::connect(&socket);
    let joined = worker.join().map_err(|_| "event fixture panicked")?;
    std::fs::remove_file(socket)?;
    joined?;
    result?;
    Ok(())
}

#[cfg(not(unix))]
fn main() -> Result<(), &'static str> {
    Err("the rewrite PTY fixture requires Unix sockets")
}
