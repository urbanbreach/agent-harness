use super::*;
use crate::app::maintenance::{loader_label, ActiveMaintenance};
use crate::app::notices::TranscriptNoticeKind;
use crate::{CompactionCommandKind, CompactionCommandOutcome};
use harness_core::event::{
    CompactionEndResult, CompactionLifecycleEvent, CompactionMethod, LifecyclePhase,
    LiveEventEnvelope, LiveEventV1, NoticeSeverity, NoticeSource, RuntimeEvent,
};

fn live(payload: LiveEventV1) -> RuntimeEvent {
    RuntimeEvent::Live(Box::new(LiveEventEnvelope {
        event_id: "lifecycle".into(),
        run_id: "test-run".into(),
        mono_ms: 0,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent-alpha".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("agent:agent-alpha".to_string()),
        payload,
    }))
}

fn lifecycle(phase: LifecyclePhase, reason: &str, action: &str) -> CompactionLifecycleEvent {
    CompactionLifecycleEvent {
        agent_id: "agent-alpha".into(),
        operation_id: "op-1".into(),
        phase,
        reason: reason.into(),
        action: action.into(),
        will_retry: false,
        aborted: false,
        skipped: false,
        error: None,
        result: None,
    }
}

fn live_app() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let output = Arc::clone(&intents);
    let app = AppState::new_live(
        None,
        false,
        Some(Arc::new(move |intent| {
            output.lock().unwrap_or_abort().push(intent);
        })),
    );
    (app, intents)
}

fn last_notice(app: &AppState) -> Option<(TranscriptNoticeKind, &str)> {
    app.transcript_notices()
        .last()
        .map(|notice| (notice.kind, notice.text.as_str()))
}

fn send(app: &mut AppState, event: CompactionLifecycleEvent) {
    app.ingest_runtime_event(live(LiveEventV1::CompactionLifecycle(event)));
}

#[test]
fn loader_labels_follow_reason_action_and_cancel_key() {
    let pass = |reason: &str, action: &str, method| ActiveMaintenance {
        operation_id: "op".into(),
        reason: reason.into(),
        action: action.into(),
        method,
    };
    for (reason, action, method, ascii, hint, expected) in [
        (
            "overflow",
            "context-full",
            None,
            false,
            Some("Ctrl+C"),
            Some("Context overflow detected, Auto context-full maintenance… (Ctrl+C to cancel)"),
        ),
        (
            "incomplete",
            "context-full",
            Some(CompactionMethod::Remote),
            false,
            None,
            Some("Response incomplete, Auto server compaction…"),
        ),
        (
            "idle",
            "idle",
            Some(CompactionMethod::Archive),
            true,
            Some("Esc"),
            Some("Idle Auto-archive... (Esc to cancel)"),
        ),
        (
            "threshold",
            "handoff",
            None,
            false,
            None,
            Some("Auto-handoff…"),
        ),
        ("threshold", "shake", None, false, None, Some("Auto-shake…")),
        (
            "manual",
            "context-full",
            None,
            true,
            Some("Ctrl+C"),
            Some("Compacting context... (Ctrl+C to cancel)"),
        ),
        (
            "manual",
            "handoff",
            None,
            false,
            Some("Ctrl+C"),
            Some("Generating handoff… (Ctrl+C to cancel)"),
        ),
        ("manual", "shake", None, false, Some("Ctrl+C"), None),
    ] {
        assert_eq!(
            loader_label(&pass(reason, action, method), ascii, hint).as_deref(),
            expected,
            "{reason}/{action}"
        );
    }
}

#[test]
fn automatic_end_branches_use_the_end_event() {
    use TranscriptNoticeKind::{Status, Warning};
    type Edit = Box<dyn Fn(&mut CompactionLifecycleEvent)>;
    let result = CompactionEndResult {
        tokens_after: Some(20_000),
        ..Default::default()
    };
    let cases: Vec<(&str, &str, Edit, Option<(TranscriptNoticeKind, &str)>)> = vec![
        (
            "aborted",
            "context-full",
            Box::new(|event| event.aborted = true),
            Some((Status, "Auto context-full maintenance cancelled")),
        ),
        (
            "aborted handoff",
            "handoff",
            Box::new(|event| event.aborted = true),
            Some((Status, "Auto-handoff cancelled")),
        ),
        (
            "error",
            "context-full",
            Box::new(|event| event.error = Some("summary failed".into())),
            Some((Warning, "summary failed")),
        ),
        (
            "skipped",
            "context-full",
            Box::new(|event| event.skipped = true),
            None,
        ),
        (
            "result",
            "context-full",
            Box::new(move |event| event.result = Some(result.clone())),
            None,
        ),
        (
            "handoff",
            "handoff",
            Box::new(|_| {}),
            Some((Status, "Auto-handoff completed")),
        ),
        (
            "shake done",
            "shake",
            Box::new(|_| {}),
            Some((Status, "Auto-shake completed")),
        ),
        (
            "shake skipped",
            "shake",
            Box::new(|event| event.skipped = true),
            None,
        ),
        (
            "shake error",
            "shake",
            Box::new(|event| event.error = Some("partial".into())),
            Some((Warning, "partial")),
        ),
        (
            "failed",
            "context-full",
            Box::new(|_| {}),
            Some((
                Warning,
                "Auto context-full maintenance failed; continuing without maintenance",
            )),
        ),
    ];
    for (name, action, edit, expected) in cases {
        let (mut app, _) = live_app();
        send(
            &mut app,
            lifecycle(LifecyclePhase::Start, "threshold", action),
        );
        assert!(app.active_maintenance().is_some(), "{name}");
        let mut end = lifecycle(LifecyclePhase::End, "threshold", action);
        edit(&mut end);
        send(&mut app, end);
        assert!(app.active_maintenance().is_none(), "{name}");
        assert_eq!(last_notice(&app), expected, "{name}");
    }
}

#[test]
fn maintenance_leaves_the_turn_busy_state_alone() {
    let (mut app, _) = live_app();
    app.ingest_event(provider_started(1, "req", "mock", "model"));
    assert!(app.active_turn_in_progress());
    send(
        &mut app,
        lifecycle(LifecyclePhase::Start, "overflow", "context-full"),
    );
    assert!(app.active_turn_in_progress());
    send(
        &mut app,
        lifecycle(LifecyclePhase::End, "overflow", "context-full"),
    );
    assert!(app.active_turn_in_progress());
}

#[test]
fn queued_input_flushes_at_the_lifecycle_end_with_the_events_retry_decision() {
    for will_retry in [false, true] {
        let (mut app, intents) = live_app();
        send(
            &mut app,
            lifecycle(LifecyclePhase::Start, "overflow", "context-full"),
        );
        app.replace_prompt_input("continue with the plan".into());
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.maintenance_queue_lanes(), (1, 0));
        assert!(intents.lock().unwrap_or_abort().is_empty());
        let mut end = lifecycle(LifecyclePhase::End, "overflow", "context-full");
        end.will_retry = will_retry;
        send(&mut app, end);
        let sent = intents.lock().unwrap_or_abort();
        let Some(UiIntent::QueuedPrompt { prompt, .. }) = sent.last() else {
            panic!("queued prompt was not flushed: {sent:?}");
        };
        assert_eq!(
            matches!(**prompt, UiIntent::SteerPrompt { .. }),
            will_retry,
            "will_retry={will_retry}"
        );
    }
}

#[test]
fn runtime_notices_become_transcript_rows_and_statuses_coalesce() {
    let (mut app, _) = live_app();
    for (severity, message) in [
        (NoticeSeverity::Info, "first"),
        (NoticeSeverity::Info, "second"),
        (NoticeSeverity::Warning, "careful"),
        (NoticeSeverity::Error, "broken"),
    ] {
        app.ingest_runtime_event(live(LiveEventV1::RuntimeNotice {
            source: NoticeSource::Compaction,
            severity,
            message: message.into(),
        }));
    }
    let rows: Vec<_> = app
        .transcript_notices()
        .iter()
        .map(|notice| (notice.kind, notice.text.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            (TranscriptNoticeKind::Status, "second"),
            (TranscriptNoticeKind::Warning, "careful"),
            (TranscriptNoticeKind::Error, "broken"),
        ]
    );
}

#[test]
fn claude_code_compaction_leaves_a_transcript_row() {
    let (mut app, _) = live_app();
    app.ingest_event(run_started(1));
    app.ingest_event(envelope(
        2,
        "req",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req".into(),
            finish_reason: "stop".into(),
            output_digest: None,
            usage: None,
            metadata: Some(ProviderRequestFinishedMetadata {
                native_compactions: vec![harness_providers::ProviderNativeCompaction {
                    pre_tokens: Some(76_242),
                    post_tokens: Some(8_869),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        }),
    ));
    assert_eq!(
        last_notice(&app),
        Some((
            TranscriptNoticeKind::Status,
            "Claude Code compacted this conversation (76K → 8.9K tokens)"
        ))
    );
}

#[test]
fn manual_command_outcomes_are_presented_by_kind() {
    use TranscriptNoticeKind::{Error, Status, Warning};
    let done = |saved: Option<&str>| CompactionCommandOutcome::Done {
        message: None,
        tokens_after: Some(4_000),
        saved_path: saved.map(str::to_owned),
        rebuild: true,
    };
    let failed = |text: &str| CompactionCommandOutcome::Failed(text.into());
    let cases = [
        (
            CompactionCommandKind::Compact,
            CompactionCommandOutcome::Cancelled,
            Some((Error, "Compaction cancelled")),
        ),
        (
            CompactionCommandKind::Handoff,
            CompactionCommandOutcome::Cancelled,
            Some((Error, "Handoff cancelled")),
        ),
        (
            CompactionCommandKind::Compact,
            failed("boom"),
            Some((Error, "Compaction failed: boom")),
        ),
        (
            CompactionCommandKind::Shake,
            failed("boom"),
            Some((Error, "Shake failed: boom")),
        ),
        (
            CompactionCommandKind::Compact,
            CompactionCommandOutcome::Warning("Already compacted".into()),
            Some((Warning, "Already compacted")),
        ),
        (CompactionCommandKind::Compact, done(None), None),
        (
            CompactionCommandKind::Handoff,
            done(Some("/tmp/handoff.md")),
            Some((Status, "Handoff document saved to: /tmp/handoff.md")),
        ),
    ];
    for (kind, outcome, expected) in cases {
        let (mut app, _) = live_app();
        app.apply_compaction_command_outcome(kind, outcome.clone());
        assert_eq!(last_notice(&app), expected, "{kind:?} {outcome:?}");
    }
    let (mut app, _) = live_app();
    app.apply_compaction_command_outcome(CompactionCommandKind::Handoff, done(None));
    assert_eq!(
        last_notice(&app),
        Some((
            TranscriptNoticeKind::Success,
            "Context handed off and compacted in place"
        ))
    );
}

#[test]
fn key_hints_come_from_the_keymap() {
    let (app, _) = live_app();
    assert_eq!(app.interrupt_key_hint(), "Ctrl+C");
    assert_eq!(app.compaction_expand_hint(), "Ctrl+Alt+O");
}
