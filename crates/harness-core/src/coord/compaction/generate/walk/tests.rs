use super::*;
use std::{cell::RefCell, collections::VecDeque};
use tokio::time::{Duration, Instant};

const SETTINGS: CompactionRetrySettings = CompactionRetrySettings {
    enabled: true,
    max_retries: 3,
    base_delay_ms: 500,
};

fn error(kind: CompactionErrorKind, message: &str) -> CompactionError {
    CompactionError::new(kind, message)
}

fn auth() -> CompactionError {
    CompactionError {
        auth: true,
        ..error(CompactionErrorKind::Summary, "401")
    }
}

fn native(message: &str) -> CompactionError {
    error(CompactionErrorKind::Native, message)
}

fn transient() -> CompactionError {
    CompactionError {
        category: Some(harness_providers::ProviderErrorCategory::RateLimited),
        ..error(CompactionErrorKind::Summary, "slow down")
    }
}

fn hinted(ms: u64) -> CompactionError {
    let mut failure = transient();
    failure
        .headers
        .insert("retry-after-ms".into(), ms.to_string());
    failure
}

/// One scripted candidate: its provider, whether it has credentials and native routes, and
/// the result of each successive attempt (`None` is success).
struct Slot {
    provider: &'static str,
    model: &'static str,
    credential: bool,
    native: bool,
    script: Vec<Option<CompactionError>>,
}

fn slot(provider: &'static str, model: &'static str, script: Vec<Option<CompactionError>>) -> Slot {
    Slot {
        provider,
        model,
        credential: true,
        native: false,
        script,
    }
}

struct Run {
    result: Result<String, CompactionError>,
    calls: Vec<String>,
    elapsed: Duration,
}

async fn run(mode: WalkMode, retry: &CompactionRetrySettings, slots: Vec<Slot>) -> Run {
    let credentials = CompactionError {
        auth: true,
        ..error(CompactionErrorKind::Summary, "credentials hint")
    };
    let cancel = CancellationToken::new();
    let plan = WalkPlan {
        mode,
        retry,
        cancel: &cancel,
        credentials,
    };
    let candidates: Vec<Candidate> = slots
        .iter()
        .map(|slot| Candidate {
            provider: slot.provider.into(),
            model: slot.model.into(),
            window: None,
            native_capable: slot.native,
            credential: None,
        })
        .collect();
    let allowed: Vec<_> = slots.iter().map(|slot| slot.credential).collect();
    let scripts: RefCell<Vec<VecDeque<Option<CompactionError>>>> =
        RefCell::new(slots.into_iter().map(|s| s.script.into()).collect());
    let calls = RefCell::new(Vec::new());
    let start = Instant::now();
    let result = walk(
        &plan,
        candidates.clone(),
        |candidate: &Candidate| {
            let index = candidates.iter().position(|c| c.model == candidate.model);
            let credential = index
                .filter(|index| allowed[*index])
                .map(|_| CompactionCredential::default());
            async move { Ok(credential) }
        },
        |candidate: &Candidate, attempt: u32| {
            calls
                .borrow_mut()
                .push(format!("{}#{attempt}", candidate.model));
            let index = candidates
                .iter()
                .position(|c| c.model == candidate.model)
                .unwrap_or_default();
            let next = scripts.borrow_mut()[index].pop_front().flatten();
            let model = candidate.model.clone();
            async move { next.map_or(Ok(model), Err) }
        },
    )
    .await;
    Run {
        result,
        calls: calls.into_inner(),
        elapsed: start.elapsed(),
    }
}

fn message(run: &Run) -> String {
    match &run.result {
        Ok(model) => format!("ok:{model}"),
        Err(error) => error.message.clone(),
    }
}

#[tokio::test(start_paused = true)]
async fn credentials_gate_candidates_and_pick_the_terminal_error() {
    for (mode, expected) in [
        (WalkMode::Automatic, "Compaction failed: no available model"),
        (WalkMode::Manual, "credentials hint"),
        (WalkMode::Speculative, "credentials hint"),
    ] {
        let mut unauthenticated = slot("a", "m1", vec![None]);
        unauthenticated.credential = false;
        let run = run(mode, &SETTINGS, vec![unauthenticated]).await;
        assert_eq!(message(&run), expected);
        assert!(run.calls.is_empty(), "no request without credentials");
    }
    // An authentication failure advances to a working candidate, or reports the hint.
    let run_ok = run(
        WalkMode::Automatic,
        &SETTINGS,
        vec![
            slot("a", "m1", vec![Some(auth())]),
            slot("b", "m2", vec![None]),
        ],
    )
    .await;
    assert_eq!(message(&run_ok), "ok:m2");
    for mode in [WalkMode::Automatic, WalkMode::Manual] {
        let run = run(mode, &SETTINGS, vec![slot("a", "m1", vec![Some(auth())])]).await;
        assert_eq!(message(&run), "credentials hint");
    }
}

#[tokio::test(start_paused = true)]
async fn native_failures_bind_later_candidates_to_the_same_provider_route() {
    let capable = |model, script| Slot {
        native: true,
        ..slot("p", model, script)
    };
    for mode in [WalkMode::Automatic, WalkMode::Manual] {
        // The first native failure survives a later same-provider auth failure.
        let preserved = run(
            mode,
            &SETTINGS,
            vec![
                capable("m1", vec![Some(native("first native"))]),
                capable("m2", vec![Some(auth())]),
            ],
        )
        .await;
        assert_eq!(message(&preserved), "first native");
        // A native-incapable or cross-provider candidate is never reached.
        for next in [slot("p", "m2", vec![None]), slot("q", "m2", vec![None])] {
            let blocked = run(
                mode,
                &SETTINGS,
                vec![capable("m1", vec![Some(native("first native"))]), next],
            )
            .await;
            assert_eq!(message(&blocked), "first native");
            assert_eq!(blocked.calls, ["m1#0"]);
        }
        // The same provider's native-capable candidate may continue.
        let continued = run(
            mode,
            &SETTINGS,
            vec![
                capable("m1", vec![Some(native("first native"))]),
                capable("m2", vec![None]),
            ],
        )
        .await;
        assert_eq!(message(&continued), "ok:m2");
    }
}

#[tokio::test(start_paused = true)]
async fn manual_walks_never_back_off_and_propagate_ordinary_failures() {
    for mode in [WalkMode::Manual, WalkMode::Speculative] {
        let run = run(
            mode,
            &SETTINGS,
            vec![
                slot("a", "m1", vec![Some(transient())]),
                slot("a", "m2", vec![None]),
            ],
        )
        .await;
        assert_eq!(message(&run), "slow down");
        assert_eq!(run.calls, ["m1#0"]);
        assert_eq!(run.elapsed, Duration::ZERO);
    }
}

#[tokio::test(start_paused = true)]
async fn automatic_retries_double_from_the_base_and_follow_larger_hints() {
    // 500 ms then 1000 ms, then success on the third attempt.
    let waited = run(
        WalkMode::Automatic,
        &SETTINGS,
        vec![slot(
            "a",
            "m1",
            vec![Some(transient()), Some(transient()), None],
        )],
    )
    .await;
    assert_eq!(message(&waited), "ok:m1");
    assert_eq!(waited.calls, ["m1#0", "m1#1", "m1#2"]);
    assert_eq!(waited.elapsed, Duration::from_millis(1500));
    // A hint above the doubled base wins; a zero hint still qualifies for a retry.
    for (failure, wait) in [(hinted(2000), 2000), (hinted(0), 500)] {
        let run = run(
            WalkMode::Automatic,
            &SETTINGS,
            vec![slot("a", "m1", vec![Some(failure), None])],
        )
        .await;
        assert_eq!(run.elapsed, Duration::from_millis(wait));
    }
    // Retries stop at the configured maximum and then advance.
    let exhausted = run(
        WalkMode::Automatic,
        &SETTINGS,
        vec![
            slot("a", "m1", vec![Some(transient()); 4]),
            slot("a", "m2", vec![None]),
        ],
    )
    .await;
    assert_eq!(message(&exhausted), "ok:m2");
    assert_eq!(
        exhausted
            .calls
            .iter()
            .filter(|c| c.starts_with("m1"))
            .count(),
        4
    );
}

#[tokio::test(start_paused = true)]
async fn automatic_walks_skip_waiting_when_retry_is_ruled_out() {
    let overflow = CompactionError {
        category: Some(harness_providers::ProviderErrorCategory::ContextWindowExceeded),
        ..error(CompactionErrorKind::Summary, "too long")
    };
    let timeout = error(
        CompactionErrorKind::Summary,
        "request timed out (retry-after-ms=900)",
    );
    let disabled = CompactionRetrySettings {
        enabled: false,
        ..SETTINGS
    };
    for (failure, settings) in [
        (overflow, SETTINGS),
        (timeout, SETTINGS),
        (transient(), disabled),
    ] {
        let run = run(
            WalkMode::Automatic,
            &settings,
            vec![
                slot("a", "m1", vec![Some(failure)]),
                slot("a", "m2", vec![None]),
            ],
        )
        .await;
        assert_eq!(message(&run), "ok:m2");
        assert_eq!(run.calls, ["m1#0", "m2#0"]);
        assert_eq!(run.elapsed, Duration::ZERO);
    }
}

#[tokio::test(start_paused = true)]
async fn long_waits_advance_unless_the_candidate_is_last() {
    // A wait above 30 s moves on when a later slot exists, even one without credentials.
    let mut unauthenticated = slot("a", "m2", vec![None]);
    unauthenticated.credential = false;
    let skipped = run(
        WalkMode::Automatic,
        &SETTINGS,
        vec![slot("a", "m1", vec![Some(hinted(30_001))]), unauthenticated],
    )
    .await;
    assert_eq!(message(&skipped), "slow down");
    assert_eq!(skipped.elapsed, Duration::ZERO);
    // Exactly 30 s still waits; the last candidate waits out any hint.
    for hint in [30_000, 120_000] {
        let waited = run(
            WalkMode::Automatic,
            &SETTINGS,
            vec![slot("a", "m1", vec![Some(hinted(hint)), None])],
        )
        .await;
        assert_eq!(message(&waited), "ok:m1");
        assert_eq!(waited.elapsed, Duration::from_millis(hint));
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_preflight_or_cancel_ends_the_walk_without_attempts() {
    let credentials = error(CompactionErrorKind::Summary, "hint");
    let cancel = CancellationToken::new();
    let plan = WalkPlan {
        mode: WalkMode::Automatic,
        retry: &SETTINGS,
        cancel: &cancel,
        credentials,
    };
    let candidates = vec![Candidate {
        provider: "a".into(),
        model: "m1".into(),
        window: None,
        native_capable: false,
        credential: None,
    }];
    let result: Result<(), _> = walk(
        &plan,
        candidates.clone(),
        |_: &Candidate| async { Err(native("lookup failed")) },
        |_: &Candidate, _| async { Ok(()) },
    )
    .await;
    assert_eq!(
        result.err().map(|e| e.message),
        Some("lookup failed".into())
    );
    cancel.cancel();
    let result: Result<(), _> = walk(
        &plan,
        candidates,
        |_: &Candidate| async { Ok(Some(CompactionCredential::default())) },
        |_: &Candidate, _| async { Ok(()) },
    )
    .await;
    assert!(result.err().is_some_and(|e| is_cancelled(&e)));
}
