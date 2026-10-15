use super::*;
use tokio_stream::StreamExt;

/// A coordinator whose second turn is rejected with a bare 413, with the third scripted
/// response following `after`.
async fn rejected_with_bare_413(
    enabled: bool,
    after: Vec<Vec<Stream>>,
) -> Result<
    (
        Arc<MockProvider>,
        CoordinatorHandle,
        RunInfo,
        String,
        tempfile::TempDir,
    ),
    Box<dyn std::error::Error>,
> {
    let temp = tempfile::tempdir()?;
    let mut script = vec![
        answer("old answer ".repeat(1000)),
        answer("latest answer"),
        vec![Stream::error("provider returned HTTP 413")],
    ];
    script.extend(after);
    let provider = Arc::new(MockProvider::script(script));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::clone(&provider) as Arc<dyn harness_providers::Provider>;
    config.compaction.method_order = vec![CompactionMethod::Soft];
    config.compaction.async_enabled = false;
    config.compaction.keep_recent_tokens = 100;
    config.compaction.reserve_tokens = Some(0);
    config.compaction.fallback_input_tokens = 32_000;
    config.compaction.threshold_tokens = std::num::NonZeroU32::new(u32::MAX);
    config.compaction.enabled = false;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("bare 413", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(handle::system(), "default", None)
        .await?;
    for prompt in ["old request ".repeat(1000), "latest request".into()] {
        let turn = coordinator
            .request_agent_turn(handle::system(), &agent, prompt)
            .await?;
        settled(&coordinator, &turn).await?;
    }
    coordinator
        .call(move |s| {
            s.config.compaction.enabled = enabled;
            Ok(())
        })
        .await?;
    Ok((provider, coordinator, run, agent, temp))
}

/// A 413 without a body is a size rejection, not a token overflow: with nothing able to
/// shrink the request the turn stops after one byte-shaped warning and no retry.
#[tokio::test]
async fn bare_413_without_usable_compaction_warns_about_bytes_and_stops(
) -> Result<(), Box<dyn std::error::Error>> {
    let (provider, coordinator, run, agent, _temp) =
        rejected_with_bare_413(false, Vec::new()).await?;
    let mut live = coordinator.event_store().await?.subscribe_runtime(1)?;
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "continue")
        .await?;
    assert!(settled(&coordinator, &turn).await.is_err());
    let warned = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        live.any(|event| {
            matches!(event, Ok(RuntimeEvent::Live(e)) if matches!(&e.payload,
                LiveEventV1::RuntimeNotice { message, .. } if message.contains("HTTP 413")))
        }),
    )
    .await?;
    assert!(warned);
    coordinator.stop_run().await?;
    assert_eq!(provider.call_count(), 3);
    let events = crate::store::read_events(&run.events_path)?;
    assert!(!events
        .iter()
        .any(|e| matches!(e.payload, EventV1::SessionCompaction(_))));
    Ok(())
}

/// The same rejection with a usable method shrinks older history and retries once.
#[tokio::test]
async fn bare_413_with_usable_compaction_summarizes_and_retries(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_provider, coordinator, run, agent, _temp) = rejected_with_bare_413(
        true,
        vec![
            answer(SUMMARY),
            answer("Rewrite checkpoint"),
            answer("continued"),
        ],
    )
    .await?;
    let turn = coordinator
        .request_agent_turn(handle::system(), &agent, "continue")
        .await?;
    settled(&coordinator, &turn).await?;
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    let triggers: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.payload {
            EventV1::SessionCompaction(c) => Some(c.trigger_reason.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(triggers, ["overflow"]);
    Ok(())
}
