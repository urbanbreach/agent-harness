use std::sync::Arc;

use harness_core::event::{ActorKind, EventEnvelopeV1, RuntimeEvent};
use harness_core::store::{EventStore, EventStoreError};
use harness_tui::{LiveUpdate, LiveUpdateSender};

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
    live_update_tx: LiveUpdateSender,
    start_from_seq: u64,
    _live_agent_target: Option<LiveAgentTargetState>,
    stop_after_terminal_event: bool,
) -> Result<(), String> {
    let mut from_seq = start_from_seq.max(1);
    let mut last_seq_seen = from_seq.saturating_sub(1);

    loop {
        let mut stream = store
            .subscribe_runtime(from_seq)
            .map_err(|err| err.to_string())?;
        let mut should_resubscribe = false;

        while let Some(next) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            match next {
                Ok(RuntimeEvent::Live(event)) => {
                    if live_update_tx
                        .send(LiveUpdate::Event(Box::new(RuntimeEvent::Live(event))))
                        .is_err()
                    {
                        return Ok(());
                    }
                }
                Ok(RuntimeEvent::Durable(event)) => {
                    if forward_durable_event(
                        event,
                        &live_update_tx,
                        &mut last_seq_seen,
                        stop_after_terminal_event,
                    ) {
                        return Ok(());
                    }
                    from_seq = last_seq_seen.saturating_add(1);
                }
                Err(EventStoreError::SubscriberLagged(skipped)) => {
                    let _ = live_update_tx.send(LiveUpdate::Status(format!(
                        "live stream lagged by {skipped}; replaying from seq {}",
                        last_seq_seen.saturating_add(1)
                    )));

                    if replay_events_to_tui(
                        store.as_ref(),
                        &live_update_tx,
                        &mut last_seq_seen,
                        stop_after_terminal_event,
                    )
                    .await?
                    {
                        return Ok(());
                    }
                    from_seq = last_seq_seen.saturating_add(1);

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

async fn replay_events_to_tui(
    store: &dyn EventStore,
    live_update_tx: &LiveUpdateSender,
    last_seq_seen: &mut u64,
    stop_after_terminal_event: bool,
) -> Result<bool, String> {
    let mut replay = store
        .replay(last_seq_seen.saturating_add(1))
        .map_err(|err| err.to_string())?;
    while let Some(replayed) = std::future::poll_fn(|cx| replay.as_mut().poll_next(cx)).await {
        let event = replayed.map_err(|err| err.to_string())?;
        if forward_durable_event(
            Box::new(event),
            live_update_tx,
            last_seq_seen,
            stop_after_terminal_event,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn forward_durable_event(
    event: Box<EventEnvelopeV1>,
    live_update_tx: &LiveUpdateSender,
    last_seq_seen: &mut u64,
    stop_after_terminal_event: bool,
) -> bool {
    if event.seq <= *last_seq_seen {
        return false;
    }
    let terminal_event = is_terminal_event(&event.payload);
    *last_seq_seen = event.seq;
    live_update_tx
        .send(LiveUpdate::Event(Box::new(RuntimeEvent::Durable(event))))
        .is_err()
        || (stop_after_terminal_event && terminal_event)
}
