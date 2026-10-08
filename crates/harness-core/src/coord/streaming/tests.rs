use super::*;
use crate::{clock::FakeClock, redact::DefaultRedactor};
use harness_providers::{mock::MockProvider, MessageRole};
use std::sync::atomic::{AtomicUsize, Ordering};

const PASSAGE: &str = "I need to examine the current implementation and decide which concrete change should happen next. ";
const BILLED_USAGE: CompletionUsage = CompletionUsage {
    prompt_tokens: 7,
    completion_tokens: 3,
    total_tokens: 10,
};

/// A scripted provider whose abort handshake retains billed usage.
struct AbortableMock {
    inner: Arc<MockProvider>,
    aborted: Arc<AtomicUsize>,
    billed_usage: Option<CompletionUsage>,
}

struct AbortableStream {
    inner: ProviderEventStream,
    abort: CancellationToken,
    aborted: Arc<AtomicUsize>,
    settled: bool,
    billed_usage: Option<CompletionUsage>,
}

impl tokio_stream::Stream for AbortableStream {
    type Item = Stream;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Stream>> {
        if self.settled {
            return std::task::Poll::Ready(None);
        }
        if self.abort.is_cancelled() {
            self.settled = true;
            self.aborted.fetch_add(1, Ordering::SeqCst);
            return std::task::Poll::Ready(Some(Stream::Aborted {
                usage: self.billed_usage.clone(),
            }));
        }
        self.inner.as_mut().poll_next(cx)
    }
}

#[async_trait::async_trait]
impl Provider for AbortableMock {
    async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
        self.inner.stream_completion(request).await
    }

    async fn stream_completion_abortable(
        &self,
        request: CompletionRequest,
        abort: CancellationToken,
    ) -> ProviderEventStream {
        Box::pin(AbortableStream {
            inner: self.stream_completion(request).await,
            abort,
            aborted: Arc::clone(&self.aborted),
            settled: false,
            billed_usage: self.billed_usage.clone(),
        })
    }
}

fn looping(reasoning: bool) -> Vec<Stream> {
    let mut events: Vec<_> = (0..80)
        .map(|_| {
            if reasoning {
                Stream::ReasoningDelta(PASSAGE.into())
            } else {
                Stream::TextDelta(PASSAGE.into())
            }
        })
        .collect();
    events.push(Stream::TextDelta("End of stream.".into()));
    events.push(Stream::Done { usage: None });
    events
}

struct Outcome {
    events: Vec<EventEnvelopeV1>,
    requests: Vec<CompletionRequest>,
    aborted: usize,
    failure: Option<String>,
    usage: Vec<crate::subagent::FinalizedProviderUsage>,
}

async fn run(
    scripts: Vec<Vec<Stream>>,
    enabled: bool,
    max_retries: u32,
    billed_usage: Option<CompletionUsage>,
) -> Result<Outcome, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let inner = Arc::new(MockProvider::script(scripts));
    let aborted = Arc::new(AtomicUsize::new(0));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::new(AbortableMock {
        inner: Arc::clone(&inner),
        aborted: Arc::clone(&aborted),
        billed_usage,
    });
    config.behavior.stream_guard.enabled = enabled;
    config.behavior.stream_guard.max_retries = max_retries;
    config.provider_retry.max_retries = 5;
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator.start_run("stream-guard", temp.path()).await?;
    let agent = coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, None),
            "default",
            None,
        )
        .await?;
    let turn = coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, None),
            agent.clone(),
            "finish the work",
        )
        .await?;
    let mut events = coordinator.event_store().await?.subscribe(1)?;
    let failure = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(event) = events.next().await {
            match event?.payload {
                EventV1::TaskCompleted(e) if e.task_id.as_str() == turn => return Ok(None),
                EventV1::TaskCancelled(e) if e.task_id.as_str() == turn => {
                    return Ok(Some(e.reason));
                }
                _ => {}
            }
        }
        Err(EventStoreError::Invalid("turn did not settle"))
    })
    .await??;
    let usage = coordinator
        .call(move |runtime| {
            let state = runtime
                .agents
                .get(&agent)
                .ok_or(CoordinatorError::UnknownAgent(agent))?;
            Ok(state.messages.usage.clone())
        })
        .await?;
    coordinator.stop_run().await?;
    Ok(Outcome {
        events: crate::store::read_events(&run.events_path)?,
        requests: inner.captured_requests().await,
        aborted: aborted.load(Ordering::SeqCst),
        failure,
        usage,
    })
}

#[tokio::test]
async fn stream_guard_aborts_text_and_reasoning_then_retries_with_recorded_correction(
) -> Result<(), Box<dyn std::error::Error>> {
    for (reasoning, billed_usage) in [
        (false, Some(BILLED_USAGE)),
        (true, Some(BILLED_USAGE)),
        (false, None),
    ] {
        let outcome = run(
            vec![
                looping(reasoning),
                vec![
                    Stream::TextDelta("Finished the work.".into()),
                    Stream::Done { usage: None },
                ],
            ],
            true,
            1,
            billed_usage.clone(),
        )
        .await?;
        assert_eq!(outcome.failure, None);
        assert_eq!(outcome.aborted, 1);
        assert_eq!(outcome.requests.len(), 2);
        let reminder = outcome
            .events
            .iter()
            .find_map(|event| match &event.payload {
                EventV1::RuntimeReminder(e) if e.kind == RuntimeReminderKind::StreamGuard => {
                    Some(e)
                }
                _ => None,
            })
            .ok_or("missing stream guard reminder")?;
        let correction = outcome.requests[1]
            .messages
            .last()
            .ok_or("empty retry request")?;
        assert_eq!(
            (correction.role, &correction.content),
            (MessageRole::User, &reminder.text)
        );
        assert!(!outcome.requests[1]
            .messages
            .iter()
            .any(|m| m.content.contains(PASSAGE)));
        let finished: Vec<_> = outcome
            .events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::ProviderRequestFinished(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(finished.len(), 2);
        assert_eq!(finished[0].finish_reason, "stream_guard");
        assert_eq!(finished[0].usage, billed_usage);
        assert_eq!(outcome.usage.len(), 2);
        assert_eq!(outcome.usage[0].request_id, finished[0].request_id.as_str());
        assert_eq!(outcome.usage[0].usage, billed_usage);
        assert_eq!(outcome.usage[0].usage_complete, billed_usage.is_some());
        assert_eq!(outcome.usage[1].request_id, finished[1].request_id.as_str());
        assert!(!outcome.usage[1].usage_complete);
        assert_eq!(finished[0].output_digest, None);
        assert!(finished[0]
            .metadata
            .as_ref()
            .is_some_and(|m| m.provider_error_category.is_none()));
        assert_eq!(finished[1].finish_reason, "stop");
        let assistant: Vec<_> = outcome
            .events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::AssistantMessageFinished(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].request_id, finished[1].request_id);
        let transcript = crate::transcript_projection::project_transcript(&outcome.events)?;
        for (request, state) in [
            (
                &finished[0].request_id,
                crate::transcript_projection::ProjectedMessageState::Incomplete,
            ),
            (
                &finished[1].request_id,
                crate::transcript_projection::ProjectedMessageState::Complete,
            ),
        ] {
            let message = transcript
                .messages
                .iter()
                .find(|message| {
                    message
                        .provider
                        .as_ref()
                        .and_then(|provider| provider.provider_request_id.as_deref())
                        == Some(request.as_str())
                })
                .ok_or("missing projected assistant attempt")?;
            assert_eq!(message.state, state);
        }
        assert!(outcome.events.iter().any(|event| matches!(&event.payload,
            EventV1::TaskCompleted(e) if e.result_summary == "Finished the work."
        )));
        assert!(outcome
            .events
            .iter()
            .filter_map(|event| match &event.payload {
                EventV1::ProviderRequestStarted(e) =>
                    e.metadata.as_ref().and_then(|m| m.retry.as_ref()),
                _ => None,
            })
            .all(|retry| retry.attempt == 0));
    }
    Ok(())
}

#[tokio::test]
async fn stream_guard_retry_exhaustion_fails_the_turn_without_provider_retry(
) -> Result<(), Box<dyn std::error::Error>> {
    for max_retries in [0, 1] {
        let outcome = run(
            vec![looping(false), looping(false)],
            true,
            max_retries,
            Some(BILLED_USAGE),
        )
        .await?;
        let attempts = max_retries as usize + 1;
        assert_eq!(outcome.requests.len(), attempts);
        assert_eq!(outcome.aborted, attempts);
        assert!(outcome
            .failure
            .as_deref()
            .is_some_and(|reason| reason.contains("stream guard")));
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(&event.payload,
                    EventV1::RuntimeReminder(e) if e.kind == RuntimeReminderKind::StreamGuard
                ))
                .count(),
            max_retries as usize
        );
        assert_eq!(
            outcome
                .events
                .iter()
                .filter(|event| matches!(&event.payload,
                    EventV1::ProviderRequestFinished(e) if e.finish_reason == "stream_guard"
                ))
                .count(),
            attempts
        );
        assert!(!outcome
            .events
            .iter()
            .any(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_))));
    }
    Ok(())
}

#[tokio::test]
async fn disabled_stream_guard_lets_the_same_looping_stream_finish(
) -> Result<(), Box<dyn std::error::Error>> {
    let outcome = run(vec![looping(false)], false, 1, Some(BILLED_USAGE)).await?;
    assert_eq!(outcome.failure, None);
    assert_eq!(outcome.requests.len(), 1);
    assert_eq!(outcome.aborted, 0);
    assert!(!outcome.events.iter().any(|event| matches!(&event.payload,
        EventV1::RuntimeReminder(e) if e.kind == RuntimeReminderKind::StreamGuard
    )));
    assert!(outcome.events.iter().any(|event| matches!(&event.payload,
        EventV1::ProviderRequestFinished(e) if e.finish_reason == "stop"
    )));
    assert!(outcome.events.iter().any(|event| matches!(&event.payload,
        EventV1::TaskCompleted(e) if e.result_summary.contains(PASSAGE)
    )));
    Ok(())
}
