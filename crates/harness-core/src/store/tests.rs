use super::*;
use crate::event::*;
use std::{fs, io::Write, time::Duration};
use tokio_stream::StreamExt;

fn message(id: &str) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: id.into(),
        seq: 0,
        run_id: "run".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::User, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: id.into(),
            text: "hello".into(),
        }),
    }
}

#[tokio::test]
async fn journal_preserves_sequence_subscription_and_exclusive_reopen(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let log_path = temp.path().join("private.log");
    open_private_append(&log_path)?.write_all(b"first\n")?;
    open_private_append(&log_path)?.write_all(b"second\n")?;
    assert_eq!(fs::read(&log_path)?, b"first\nsecond\n");
    #[cfg(unix)]
    {
        let link = temp.path().join("linked.log");
        std::os::unix::fs::symlink(&log_path, &link)?;
        assert!(open_private_append(&link).is_err());
        assert_eq!(crate::auth::credential_file_mode(&log_path)? & 0o777, 0o600);
    }
    assert!(JsonlFileEventStore::open(temp.path(), "../escape", true).is_err());
    assert!(JsonlFileEventStore::open_existing(temp.path(), "missing", true).is_err());
    assert!(!temp.path().join("missing").exists());
    let store = JsonlFileEventStore::open(temp.path(), "run", true)?;
    assert!(JsonlFileEventStore::open(temp.path(), "run", true).is_err());
    let mut foreign = message("wrong");
    foreign.run_id = "foreign".into();
    assert!(store.append(foreign.into()).is_err());
    for payload in [
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "a".into(),
            delta: "live text".into(),
        }),
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "a".into(),
            delta: "private reasoning".into(),
        }),
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: "a".into(),
            tool_call_count: 0,
            parts: vec![crate::session::AssistantPart::Reasoning {
                text: "private reasoning".into(),
            }],
            provenance: None,
            assistant_message: None,
        }),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "a".into(),
            finish_reason: "stop".into(),
            output_digest: None,
            usage: None,
            metadata: Some(ProviderRequestFinishedMetadata {
                thinking: Some(ProviderThinkingMetadata {
                    summary: Some("private reasoning".into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        }),
    ] {
        let mut event = message("legacy");
        event.payload = payload;
        assert!(store.append(event.into()).is_err());
    }
    assert_eq!(store.append(message("a").into())?.seq, 1);
    let mut subscription = store.subscribe(1)?;
    assert_eq!(store.append(message("b").into())?.seq, 2);
    for expected in [1, 2] {
        let event = tokio::time::timeout(Duration::from_secs(1), subscription.next())
            .await?
            .ok_or("subscription closed")??;
        assert_eq!(event.seq, expected);
    }
    store.publish_live(LiveEventEnvelope {
        event_id: "live".into(),
        run_id: "run".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, None),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: LiveEventV1::ProviderReasoningDelta {
            request_id: "a".into(),
            delta: "private-reasoning".into(),
        },
    });
    drop(subscription);
    drop(store);
    let path = temp.path().join("run/events.jsonl");
    let original = fs::read(&path)?;
    assert!(!String::from_utf8_lossy(&original).contains("private-reasoning"));
    let store = JsonlFileEventStore::open_existing(temp.path(), "run", true)?;
    let replay = store.replay(2)?.collect::<Vec<_>>().await;
    assert_eq!(replay.len(), 1);
    assert_eq!(
        replay
            .into_iter()
            .next()
            .ok_or("missing replay event")??
            .seq,
        2
    );
    assert_eq!(store.append(message("c").into())?.seq, 3);
    drop(store);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)?
        .write_all(b"{torn")?;
    let damaged = fs::read(&path)?;
    assert!(JsonlFileEventStore::open_existing(temp.path(), "run", true).is_err());
    assert_eq!(fs::read(&path)?, damaged);
    Ok(())
}
