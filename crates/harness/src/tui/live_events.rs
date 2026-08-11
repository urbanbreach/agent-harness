use std::sync::mpsc as std_mpsc;
use std::sync::Arc;

use harness_core::event::{ActorKind, EventEnvelopeV1};
use harness_core::store::{EventStore, EventStoreError};
use harness_tui::LiveUpdate;

use super::live_intents::LiveAgentTargetState;
use harness_core::event::EventV1;

pub(super) fn is_terminal_event(payload: &EventV1) -> bool {
    matches!(payload, EventV1::RunFinished(_) | EventV1::RunFailed(_))
}

pub(super) fn latest_request_id_for_agent(
    historical_events: &[EventEnvelopeV1],
    agent_id: &str,
) -> Option<String> {
    historical_events.iter().rev().find_map(|event| {
        (event.actor.kind == ActorKind::Worker && event.actor.agent_id.as_deref() == Some(agent_id))
            .then(|| event.correlation_id.clone())
            .flatten()
    })
}

pub(super) async fn forward_events_to_tui(
    store: Arc<dyn EventStore>,
    live_update_tx: std_mpsc::Sender<LiveUpdate>,
    start_from_seq: u64,
    _live_agent_target: Option<LiveAgentTargetState>,
    stop_after_terminal_event: bool,
) -> Result<(), String> {
    let mut from_seq = start_from_seq.max(1);
    let mut last_seq_seen = from_seq.saturating_sub(1);

    loop {
        let mut stream = store.subscribe(from_seq).map_err(|err| err.to_string())?;
        let mut should_resubscribe = false;

        while let Some(next) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match next {
                Ok(event) => {
                    if event.seq <= last_seq_seen {
                        continue;
                    }

                    let terminal_event = is_terminal_event(&event.payload);
                    last_seq_seen = event.seq;
                    from_seq = last_seq_seen.saturating_add(1);
                    if live_update_tx
                        .send(LiveUpdate::Event(Box::new(event)))
                        .is_err()
                    {
                        return Ok(());
                    }
                    if stop_after_terminal_event && terminal_event {
                        return Ok(());
                    }
                }
                Err(EventStoreError::SubscriberLagged(skipped)) => {
                    let _ = live_update_tx.send(LiveUpdate::Status(format!(
                        "live stream lagged by {skipped}; replaying from seq {}",
                        last_seq_seen.saturating_add(1)
                    )));

                    let mut replay = store
                        .replay(last_seq_seen.saturating_add(1))
                        .map_err(|err| err.to_string())?;
                    while let Some(replayed) =
                        std::future::poll_fn(|cx| replay.as_mut().poll_next(cx)).await
                    {
                        let replayed_event = replayed.map_err(|err| err.to_string())?;
                        if replayed_event.seq <= last_seq_seen {
                            continue;
                        }

                        let terminal_event = is_terminal_event(&replayed_event.payload);
                        last_seq_seen = replayed_event.seq;
                        from_seq = last_seq_seen.saturating_add(1);
                        if live_update_tx
                            .send(LiveUpdate::Event(Box::new(replayed_event)))
                            .is_err()
                        {
                            return Ok(());
                        }
                        if stop_after_terminal_event && terminal_event {
                            return Ok(());
                        }
                    }

                    should_resubscribe = true;
                    break;
                }
                Err(err) => {
                    return Err(format!("live stream error: {err}"));
                }
            }
        }

        if should_resubscribe {
            continue;
        }

        break;
    }

    Ok(())
}
