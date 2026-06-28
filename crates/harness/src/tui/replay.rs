use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use harness_core::event::{EventEnvelopeV1, EventV1};
#[cfg(test)]
use harness_tui::app::LaunchMetadata;
use harness_tui::{
    run_tui_with_options, set_pending_replay_launch_metadata, TuiMode, TuiOptions, UiIntent,
};

use crate::cli_io::load_events_from_run_dir;

use super::launch_metadata::replay_launch_metadata;
use super::session_history::load_recorded_runtime_context;
use super::workflow::{
    capture_first_workflow, live_workflow_from_intent, take_selected_workflow_or,
    InteractiveWorkflow,
};

pub(super) fn execute_replay_mode(
    run_dir: &Path,
    exit_on_finish: bool,
    stderr: &mut dyn Write,
) -> ExitCode {
    let events = match load_events_from_run_dir(run_dir) {
        Ok(events) => events,
        Err(err) => {
            let _ = writeln!(stderr, "replay setup failed: {err}");
            return ExitCode::from(2);
        }
    };

    if exit_on_finish && has_terminal_event(&events) {
        return ExitCode::SUCCESS;
    }

    set_pending_replay_launch_metadata(Some(replay_launch_metadata_for_run(run_dir, &events)));

    if let Err(err) = run_tui_with_options(TuiOptions {
        mode: TuiMode::Replay {
            run_dir: run_dir.to_path_buf(),
            events,
        },
        exit_on_finish,
        on_ui_intent: None,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
    }) {
        let _ = writeln!(stderr, "TUI error: {err}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}

pub(super) async fn run_replay_tui(
    run_dir: PathBuf,
    exit_on_finish: bool,
) -> Result<InteractiveWorkflow, String> {
    let events = load_events_from_run_dir(&run_dir).map_err(|err| err.to_string())?;
    set_pending_replay_launch_metadata(Some(replay_launch_metadata_for_run(&run_dir, &events)));
    let selected_workflow = Arc::new(Mutex::new(None::<InteractiveWorkflow>));
    let selected_workflow_sink = Arc::clone(&selected_workflow);
    let on_ui_intent = Arc::new(move |intent: UiIntent| {
        if let Some(workflow) = live_workflow_from_intent(&intent) {
            capture_first_workflow(&selected_workflow_sink, workflow);
        }
    });

    tokio::task::spawn_blocking(move || {
        run_tui_with_options(TuiOptions {
            mode: TuiMode::Replay { run_dir, events },
            exit_on_finish,
            on_ui_intent: Some(on_ui_intent),
            keybindings: None,
            toggles: None,
            preserve_terminal_on_exit: true,
        })
    })
    .await
    .map_err(|err| format!("replay tui task failed: {err}"))?
    .map_err(|err| format!("replay tui error: {err}"))?;

    take_selected_workflow_or(&selected_workflow, InteractiveWorkflow::Startup)
}

fn has_terminal_event(events: &[EventEnvelopeV1]) -> bool {
    events.iter().any(|event| is_terminal_event(&event.payload))
}

fn replay_launch_metadata_for_run(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> harness_tui::app::LaunchMetadata {
    replay_launch_metadata(
        load_recorded_runtime_context(run_dir).as_ref(),
        historical_events,
    )
}

pub(super) fn is_terminal_event(payload: &EventV1) -> bool {
    matches!(payload, EventV1::RunFinished(_) | EventV1::RunFailed(_))
}

#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn replay_launch_metadata_for_test(
    run_dir: &Path,
    historical_events: &[EventEnvelopeV1],
) -> LaunchMetadata {
    replay_launch_metadata_for_run(run_dir, historical_events)
}
