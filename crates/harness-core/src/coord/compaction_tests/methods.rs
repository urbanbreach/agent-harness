use super::*;
use harness_compaction::ManualMode;

async fn vision_coordinator(
    temp: &std::path::Path,
    provider: Arc<dyn harness_providers::Provider>,
    order: Vec<CompactionMethod>,
) -> Result<(CoordinatorConfig, CoordinatorHandle, RunInfo, String), Box<dyn std::error::Error>> {
    let settings = crate::config::load_config_from_str(
        r#"{ model: 'local/gpt-4.1', provider: { local: {
        type: 'openai_compatible', baseUrl: 'https://example.invalid',
        models: { 'gpt-4.1': { limit: { context: 200000, output: 8000 }, modalities: { input: ['text', 'image'] } } }
    } } }"#,
    )?;
    let mut config = CoordinatorConfig::new(temp.join("sessions"));
    config.provider = provider;
    config.agent_model_targets.insert(
        "default".into(),
        crate::config::resolve_model_selection(&settings, "local/gpt-4.1", None)?.primary,
    );
    let mut profile = crate::agent::AgentProfile::fallback("default");
    profile.model_ref = "local:gpt-4.1".into();
    config.agent_profiles.insert("default".into(), profile);
    config.compaction.async_enabled = false;
    config.compaction.keep_recent_tokens = 50;
    config.compaction.method_order = order;
    let coordinator = spawn_coordinator(
        config.clone(),
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("methods", temp).await?;
    let agent = coordinator
        .spawn_agent_idle(EventActor::new(ActorKind::User, None), "default", None)
        .await?;
    Ok((config, coordinator, run, agent))
}

async fn converse(
    coordinator: &CoordinatorHandle,
    agent: &str,
    prompts: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    for prompt in prompts {
        let turn = coordinator
            .request_agent_turn(
                EventActor::new(ActorKind::User, None),
                agent,
                prompt.clone(),
            )
            .await?;
        settled(coordinator, &turn).await?;
    }
    Ok(())
}

fn compactions(events: &[EventEnvelopeV1]) -> Vec<&crate::event::SessionCompactionEvent> {
    events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::SessionCompaction(compaction) => Some(compaction),
            _ => None,
        })
        .collect()
}

fn image_count(request: &harness_providers::CompletionRequest) -> usize {
    request
        .attachments
        .values()
        .flatten()
        .filter(|a| a.mime.starts_with("image/"))
        .count()
}

/// Archive frames are durable private artifacts, ride the next request as images, and come
/// back unchanged after resume.
#[tokio::test]
async fn manual_archive_persists_frames_and_restores_them_after_resume(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    // Varied, frame-dense history: tens of thousands of tokens shrink into a few frames.
    let history = (0..12_000).fold(String::new(), |mut text, i| {
        use std::fmt::Write as _;
        let _ = write!(text, "item{i} ");
        text
    });
    let provider = Arc::new(MockProvider::script([
        answer(format!("decided {history}")),
        answer("second answer"),
        answer("after archive"),
        answer("after resume"),
    ]));
    let (config, coordinator, run, agent) = vision_coordinator(
        temp.path(),
        Arc::clone(&provider) as Arc<dyn harness_providers::Provider>,
        CompactionMethod::DEFAULT_ORDER.to_vec(),
    )
    .await?;
    converse(
        &coordinator,
        &agent,
        &[format!("plan {history}"), "next".into()],
    )
    .await?;
    let outcome = coordinator
        .compact_agent_context_with_options(
            agent.clone(),
            ManualCompactionOptions {
                reason: "manual".into(),
                mode: Some(ManualMode::Archive),
                ..Default::default()
            },
        )
        .await?;
    assert!(matches!(
        outcome,
        ManualCompactionOutcome::Compacted {
            method: CompactionMethod::Archive,
            ..
        }
    ));
    converse(&coordinator, &agent, &["continue".into()]).await?;
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    let archive = compactions(&events)
        .first()
        .and_then(|c| c.archive.clone())
        .ok_or("archive record missing")?;
    assert!(!archive.frames.is_empty());
    for frame in &archive.frames {
        assert!(run.run_dir.join(&frame.path).is_file(), "{}", frame.path);
    }
    let requests = provider.captured_requests().await;
    let live = requests.last().ok_or("missing request")?;
    assert_eq!(image_count(live), archive.frames.len());
    assert!(!live.messages.iter().any(|m| m.content.contains(&history)));

    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "archive resumed")
        .await?;
    converse(&resumed, &agent, &["after restart".into()]).await?;
    resumed.stop_run().await?;
    let requests = provider.captured_requests().await;
    let restored = requests.last().ok_or("missing resumed request")?;
    assert_eq!(image_count(restored), archive.frames.len());
    Ok(())
}

/// An archive that would not shrink the context is refused and the walk continues to the
/// next method instead of committing it.
#[tokio::test]
async fn archive_that_cannot_reduce_falls_through_to_soft() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        answer(format!("answer {}", "short note ".repeat(100))),
        answer("second answer"),
        answer(SUMMARY),
        answer(SUMMARY),
    ]));
    let (_, coordinator, run, agent) = vision_coordinator(
        temp.path(),
        Arc::clone(&provider) as Arc<dyn harness_providers::Provider>,
        vec![CompactionMethod::Archive, CompactionMethod::Soft],
    )
    .await?;
    // About 600 summarized tokens cost less than one archive frame.
    converse(
        &coordinator,
        &agent,
        &[
            format!("request {}", "short note ".repeat(100)),
            "next".into(),
        ],
    )
    .await?;
    let outcome = coordinator
        .compact_agent_context(agent.clone(), None, "manual")
        .await?;
    assert!(
        matches!(
            outcome,
            ManualCompactionOutcome::Compacted {
                method: CompactionMethod::Soft,
                ..
            }
        ),
        "{outcome:?}"
    );
    coordinator.stop_run().await?;
    let events = crate::store::read_events(&run.events_path)?;
    let committed = compactions(&events);
    assert_eq!(committed.len(), 1);
    assert!(committed[0].archive.is_none());
    Ok(())
}

/// Provider-native compaction keeps its payload in memory only; after a restart the
/// unreadable boundary is ignored and the original history returns.
#[tokio::test]
async fn native_remote_compaction_is_transient_across_restart(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(
        MockProvider::script([
            answer("first answer"),
            answer("second answer"),
            answer("after remote"),
            answer("after resume"),
        ])
        .with_native_compaction("Native summary of the session."),
    );
    let (config, coordinator, run, agent) = vision_coordinator(
        temp.path(),
        Arc::clone(&provider) as Arc<dyn harness_providers::Provider>,
        CompactionMethod::DEFAULT_ORDER.to_vec(),
    )
    .await?;
    let original = format!("original {}", "request detail ".repeat(200));
    converse(&coordinator, &agent, &[original.clone(), "next".into()]).await?;
    let outcome = coordinator
        .compact_agent_context_with_options(
            agent.clone(),
            ManualCompactionOptions {
                reason: "manual".into(),
                mode: Some(ManualMode::Remote),
                ..Default::default()
            },
        )
        .await?;
    assert!(matches!(
        outcome,
        ManualCompactionOutcome::Compacted {
            method: CompactionMethod::Remote,
            ..
        }
    ));
    assert_eq!(provider.native_call_count(), 1);
    converse(&coordinator, &agent, &["continue".into()]).await?;
    coordinator.stop_run().await?;
    let journal = std::fs::read_to_string(&run.events_path)?;
    assert!(!journal.contains("encrypted_content"));
    let requests = provider.captured_requests().await;
    let live = requests.last().ok_or("missing request")?;
    assert!(!live.messages.iter().any(|m| m.content == original));

    let resumed = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    resumed
        .resume_run(run.run_id.to_string(), "remote resumed")
        .await?;
    converse(&resumed, &agent, &["after restart".into()]).await?;
    resumed.stop_run().await?;
    let requests = provider.captured_requests().await;
    let restored = requests.last().ok_or("missing resumed request")?;
    assert!(restored.messages.iter().any(|m| m.content == original));
    Ok(())
}

/// A request-size rejection with explicit media-limit evidence is not compacted: the turn
/// stops with a size warning instead of summarizing history.
#[tokio::test]
async fn media_limit_rejection_warns_without_compacting() -> Result<(), Box<dyn std::error::Error>>
{
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script([
        answer("first answer"),
        vec![Stream::error(
            "413 payload too large: too many images (limit of 20 images)",
        )],
    ]));
    let (_, coordinator, run, agent) = vision_coordinator(
        temp.path(),
        Arc::clone(&provider) as Arc<dyn harness_providers::Provider>,
        vec![CompactionMethod::Soft],
    )
    .await?;
    converse(&coordinator, &agent, &["first".into()]).await?;
    let store = coordinator.event_store().await?;
    let mut live = store.subscribe_runtime(1)?;
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), &agent, "second")
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
    assert_eq!(provider.call_count(), 2);
    let events = crate::store::read_events(&run.events_path)?;
    assert!(compactions(&events).is_empty());
    Ok(())
}

/// A 413 whose own failed request reported more input tokens than the window is a token
/// problem: with no compaction able to run, the dead-end warning names tokens, not bytes.
/// The status comes from the envelope; the message text names none.
#[tokio::test]
async fn rejection_usage_over_the_window_warns_about_tokens(
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio_stream::StreamExt;
    let temp = tempfile::tempdir()?;
    let usage = harness_providers::CompletionUsage {
        input_tokens: 10,
        cache_read_tokens: 250_000,
        prompt_tokens: 250_010,
        ..Default::default()
    };
    let provider = Arc::new(MockProvider::script([
        answer("first answer"),
        vec![Stream::Error {
            message: "request rejected".into(),
            category: None,
            remediation: None,
            retry_after_ms: None,
            envelope: Some(Box::new(harness_providers::ProviderErrorEnvelope {
                status: Some(413),
                usage: Some(usage),
                ..Default::default()
            })),
        }],
    ]));
    let (_, coordinator, _run, agent) = vision_coordinator(
        temp.path(),
        Arc::clone(&provider) as Arc<dyn harness_providers::Provider>,
        vec![CompactionMethod::Soft],
    )
    .await?;
    converse(&coordinator, &agent, &["first".into()]).await?;
    coordinator
        .call(|s| {
            s.config.compaction.enabled = false;
            Ok(())
        })
        .await?;
    let mut live = coordinator.event_store().await?.subscribe_runtime(1)?;
    let turn = coordinator
        .request_agent_turn(EventActor::new(ActorKind::User, None), &agent, "second")
        .await?;
    assert!(settled(&coordinator, &turn).await.is_err());
    let warned = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        live.any(|event| {
            matches!(event, Ok(RuntimeEvent::Live(e)) if matches!(&e.payload,
                LiveEventV1::RuntimeNotice { message, .. }
                    if message.contains("token-context problem") && message.contains("250,010")))
        }),
    )
    .await?;
    assert!(warned);
    coordinator.stop_run().await?;
    assert_eq!(provider.call_count(), 2);
    Ok(())
}
