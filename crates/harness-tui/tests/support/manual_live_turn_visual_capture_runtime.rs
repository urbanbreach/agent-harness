use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use harness_core::event::{
    EventV1, ProviderRequestStartedEvent, RuntimeEvent, TaskCancelledEvent, TaskScheduleState,
    TaskScheduledEvent, TaskTerminalScope,
};
use harness_tui::{
    live_update_channel, run_tui_with_options, LiveUpdate, TuiMode, TuiOptions, UiIntent,
};

use crate::capture_events::{
    envelope, CaptureScenario, ACTIVE_REQUEST_ID, ACTIVE_TASK_ID, QUEUED_REQUEST_ID, QUEUED_TASK_ID,
};

pub(crate) fn run_capture(config: CaptureScenario) -> Result<(), Box<dyn std::error::Error>> {
    let run_dir = tempfile::tempdir()?;
    let (update_tx, update_rx) = live_update_channel();
    if let Some(status) = config.status {
        update_tx
            .send(LiveUpdate::Status(status.to_string()))
            .map_err(|_| std::io::Error::other("seed live status"))?;
    }

    let on_ui_intent: Option<Arc<dyn Fn(UiIntent) + Send + Sync>> =
        config.send_now_transition.then(|| {
            let transition_tx = update_tx.clone();
            let transition_step = Arc::new(AtomicU8::new(0));
            Arc::new(move |_intent: UiIntent| {
                let updates = match transition_step.fetch_add(1, Ordering::AcqRel) {
                    0 => vec![envelope(
                        9,
                        ACTIVE_REQUEST_ID,
                        EventV1::TaskCancelled(TaskCancelledEvent {
                            task_id: ACTIVE_TASK_ID.into(),
                            reason: "send_now".to_string(),
                            task_scope: Some(TaskTerminalScope::AgentTurn),
                        }),
                    )],
                    1 => vec![
                        envelope(
                            10,
                            QUEUED_REQUEST_ID,
                            EventV1::TaskScheduled(TaskScheduledEvent {
                                task_id: QUEUED_TASK_ID.into(),
                                state: TaskScheduleState::Started,
                                queue_key: Some("provider_model:mock:model-manual".to_string()),
                                metadata: None,
                            }),
                        ),
                        envelope(
                            11,
                            QUEUED_REQUEST_ID,
                            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                                request_id: QUEUED_REQUEST_ID.into(),
                                provider_id: "mock".to_string(),
                                model_id: "model-manual".to_string(),
                                prompt_summary: "Queued follow-up".to_string(),
                                request_digest: "digest-manual-queued".to_string(),
                                metadata: None,
                            }),
                        ),
                    ],
                    _ => Vec::new(),
                };
                for event in updates {
                    let _ = transition_tx.send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(
                        Box::new(event),
                    ))));
                }
            }) as Arc<dyn Fn(UiIntent) + Send + Sync>
        });

    run_tui_with_options(TuiOptions {
        mode: TuiMode::Live {
            run_dir: run_dir.path().to_path_buf(),
            historical_events: config.events,
            session_history_entries: Vec::new(),
            prompt_history_path: None,
            update_rx,
            compact_session_supported: false,
        },
        exit_on_finish: false,
        on_ui_intent,
        keybindings: None,
        toggles: None,
        preserve_terminal_on_exit: false,
        skip_alternate_screen: false,
    })
    .map_err(|error| std::io::Error::other(format!("capture TUI: {error}")))?;

    drop(update_tx);
    Ok(())
}
