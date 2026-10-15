use super::*;
use crate::{
    clock::FakeClock,
    coord::{spawn_coordinator, CoordinatorConfig},
    redact::DefaultRedactor,
};
use harness_providers::{
    mock::MockProvider, ProviderErrorEnvelope, ProviderStreamEvent as Stream,
    ProviderStreamFinishedMetadata,
};
use tokio::time::{Duration, Instant};

fn failure(message: &str, status: Option<u16>, headers: &[(&str, &str)]) -> Stream {
    let envelope = ProviderErrorEnvelope {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        body: None,
        usage: None,
    };
    Stream::Error {
        message: message.into(),
        category: None,
        remediation: None,
        retry_after_ms: None,
        envelope: status.map(|_| Box::new(envelope)),
    }
}

fn text(text: &str) -> Stream {
    Stream::TextDelta(text.into())
}

fn done() -> Stream {
    Stream::Done { usage: None }
}

/// Reads one scripted response per attempt and reports `(result, calls, virtual wait)`.
async fn read(
    script: Vec<Vec<Stream>>,
    kind: CompactionErrorKind,
    mode: RetryMode,
) -> Result<(Result<Extracted, CompactionError>, usize, Duration), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let provider = Arc::new(MockProvider::script(script));
    let mut config = CoordinatorConfig::new(temp.path().join("sessions"));
    config.provider = Arc::<MockProvider>::clone(&provider);
    let handle = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let permits = Arc::new(tokio::sync::Semaphore::new(1));
    let cancel = CancellationToken::new();
    let actor = EventActor::new(crate::event::ActorKind::Worker, None);
    let reader = Reader {
        handle: &handle,
        provider: provider.as_ref(),
        permits: &permits,
        actor: &actor,
        task: "task",
        cancel: &cancel,
        preview: None,
    };
    let start = Instant::now();
    let result = reader.run(CompletionRequest::default(), kind, mode).await;
    Ok((result, provider.call_count(), start.elapsed()))
}

#[test]
fn provider_failures_carry_exact_prefixes_and_only_supplied_status() {
    use CompactionErrorKind::{Branch, Handoff, ShortSummary, Summary, TurnPrefix};
    let envelope = |status| ProviderErrorEnvelope {
        status: Some(status),
        body: Some("body words".into()),
        ..Default::default()
    };
    for (kind, message, status, expected, expected_status) in [
        (Summary, "boom", None, "Summarization failed: boom", None),
        (
            TurnPrefix,
            "boom",
            None,
            "Turn prefix summarization failed: boom",
            None,
        ),
        (
            ShortSummary,
            "boom",
            Some(403),
            "Short summary failed: boom",
            Some(403),
        ),
        (
            Handoff,
            "",
            Some(401),
            "Handoff generation failed: Unknown error",
            Some(401),
        ),
        (Branch, "", None, "Summarization failed", None),
        (Branch, "raw", None, "raw", None),
    ] {
        let error = provider_failure(kind, message.into(), None, None, status.map(envelope));
        assert_eq!(
            (error.message.as_str(), error.status),
            (expected, expected_status)
        );
        assert_eq!(error.auth, matches!(status, Some(401 | 403)));
        // The body is kept for classifiers but never joins the user-facing message.
        assert_eq!(error.cause.is_some(), status.is_some());
    }
}

#[test]
fn classification_excludes_before_it_admits_and_merges_hints() {
    let case = |status: Option<u16>,
                category: Option<ProviderErrorCategory>,
                message: &str,
                headers: &[(&str, &str)]| {
        let mut error = provider_failure(
            CompactionErrorKind::Summary,
            message.into(),
            category,
            None,
            None,
        );
        error.status = status;
        error.headers = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        classify(&error)
    };
    use ProviderErrorCategory::{ContextWindowExceeded, MalformedStream, RateLimited};
    // (class, retryable, hint)
    for (class, retryable, hint) in [
        (case(Some(503), None, "overloaded", &[]), true, None),
        (case(Some(408), None, "x", &[]), true, None),
        (case(Some(409), None, "x", &[]), true, None),
        (case(Some(400), None, "bad request", &[]), false, None),
        (
            case(Some(429), None, "monthly usage limit reached", &[]),
            true,
            None,
        ),
        // Exclusions win over a transient status or a hint.
        (
            case(
                Some(503),
                Some(ContextWindowExceeded),
                "x",
                &[("retry-after", "1")],
            ),
            false,
            Some(1000),
        ),
        (
            case(Some(503), None, "content_filter triggered", &[]),
            false,
            None,
        ),
        (
            case(Some(413), Some(RateLimited), "too big", &[]),
            false,
            None,
        ),
        (
            case(
                None,
                Some(MalformedStream),
                "tool call arguments are not valid JSON",
                &[],
            ),
            false,
            None,
        ),
        // Hints: the larger of header and suffix, zero is a real hint.
        (
            case(
                None,
                None,
                "retry-after-ms=3000 later",
                &[("retry-after-ms", "1000")],
            ),
            true,
            Some(3000),
        ),
        (
            case(None, None, "x", &[("Retry-After", "2")]),
            true,
            Some(2000),
        ),
        (
            case(None, None, "x", &[("retry-after-ms", "0")]),
            true,
            Some(0),
        ),
    ] {
        assert_eq!(
            (class.retryable(), class.hint_ms),
            (retryable, hint),
            "{class:?}"
        );
    }
    assert!(case(None, None, "request timed out", &[]).timeout);
    assert!(case(None, Some(ContextWindowExceeded), "x", &[]).overflow);
    // Hints in the preserved envelope body count, and a deterministic local tool-call parse
    // failure is excluded even with a hint.
    let mut error = provider_failure(CompactionErrorKind::Summary, "x".into(), None, None, None);
    error.cause = Some("later retry-after-ms=4000".into());
    assert_eq!(classify(&error).hint_ms, Some(4000));
    let parse = provider_failure(
        CompactionErrorKind::Summary,
        "tool call arguments failed to parse retry-after-ms=10".into(),
        Some(MalformedStream),
        None,
        None,
    );
    assert!(!classify(&parse).retryable());
}

#[tokio::test(start_paused = true)]
async fn only_text_blocks_are_kept_and_joined_between_completed_blocks(
) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = ProviderStreamFinishedMetadata {
        output_limit_reached: true,
        ..Default::default()
    };
    for (events, expected) in [
        // Separate blocks around an ignored (here malformed) tool call.
        (
            vec![
                text("A"),
                Stream::ToolCallComplete {
                    tool_call_id: "c".into(),
                    function_name: "t".into(),
                    arguments_json: "{broken".into(),
                },
                text("B"),
                done(),
            ],
            "A\nB",
        ),
        // A completed text block ends at its boundary event even with no event between.
        (
            vec![
                text("A"),
                Stream::TextBlockEnd,
                text("B"),
                Stream::TextBlockEnd,
                done(),
            ],
            "A\nB",
        ),
        // Consecutive deltas are one block; reasoning ends a block; no trailing join.
        (
            vec![
                text("A"),
                text("1"),
                Stream::ReasoningDelta("r".into()),
                text("B"),
                Stream::ReasoningDelta("r".into()),
                done(),
            ],
            "A1\nB",
        ),
        // Hitting the output limit keeps the text.
        (
            vec![
                text("partial"),
                Stream::DoneWithMetadata {
                    usage: None,
                    metadata: Some(metadata),
                },
            ],
            "partial",
        ),
    ] {
        let (result, calls, _) = read(
            vec![events],
            CompactionErrorKind::Handoff,
            RetryMode::Default,
        )
        .await?;
        let extracted = result.map_err(|e| e.message)?;
        assert_eq!(
            (extracted.text.as_str(), extracted.aborted, calls),
            (expected, false, 1)
        );
    }
    // A provider abort that no one requested is reported, not rejected.
    let (result, ..) = read(
        vec![vec![text("so far"), Stream::Aborted { usage: None }]],
        CompactionErrorKind::Summary,
        RetryMode::Default,
    )
    .await?;
    let extracted = result.map_err(|e| e.message)?;
    assert_eq!(
        (extracted.text.as_str(), extracted.aborted),
        ("so far", true)
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn default_retry_recovers_but_disabled_and_excluded_failures_do_not(
) -> Result<(), Box<dyn std::error::Error>> {
    let overloaded = || vec![failure("overloaded", Some(529), &[])];
    let recovered = || vec![text("recovered"), done()];
    // Default retry: the second attempt succeeds after one jittered backoff (375..=500 ms).
    let (result, calls, waited) = read(
        vec![overloaded(), recovered()],
        CompactionErrorKind::Summary,
        RetryMode::Default,
    )
    .await?;
    assert_eq!(result.map_err(|e| e.message)?.text, "recovered");
    assert_eq!(calls, 2);
    assert!((Duration::from_millis(375)..=Duration::from_millis(500)).contains(&waited));
    // Disabled: one call and the prefixed original failure with its status.
    let (result, calls, _) = read(
        vec![overloaded(), recovered()],
        CompactionErrorKind::Summary,
        RetryMode::Disabled,
    )
    .await?;
    let error = result.err().ok_or("expected a failure")?;
    assert_eq!(
        (calls, error.message.as_str(), error.status),
        (1, "Summarization failed: overloaded", Some(529))
    );
    // A hint above the maximum wait stops without waiting; a small hint is honored.
    let hinted = |ms: &str| vec![failure("overloaded", Some(529), &[("retry-after-ms", ms)])];
    let (result, calls, waited) = read(
        vec![hinted("31000"), recovered()],
        CompactionErrorKind::Summary,
        RetryMode::Default,
    )
    .await?;
    assert_eq!((result.is_err(), calls, waited), (true, 1, Duration::ZERO));
    let (result, calls, waited) = read(
        vec![hinted("1000"), recovered()],
        CompactionErrorKind::Summary,
        RetryMode::Default,
    )
    .await?;
    assert_eq!(
        (result.is_ok(), calls, waited),
        (true, 2, Duration::from_millis(1000))
    );
    // Overflow is never retried even with a transient status.
    let overflow = Stream::Error {
        message: "context too long".into(),
        category: Some(ProviderErrorCategory::ContextWindowExceeded),
        remediation: None,
        retry_after_ms: Some(10),
        envelope: None,
    };
    let (result, calls, _) = read(
        vec![vec![overflow], recovered()],
        CompactionErrorKind::Summary,
        RetryMode::Default,
    )
    .await?;
    assert_eq!((result.is_err(), calls), (true, 1));
    Ok(())
}
