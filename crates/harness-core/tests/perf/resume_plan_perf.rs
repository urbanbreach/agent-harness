use harness_core::UnwrapOrAbort;
use std::time::{Duration, Instant};

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1,
    ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, SCHEMA_VERSION,
};
use harness_core::proj::project_resume_plan;

const RUN_ID: &str = "run_perf";
const AGENT_ID: &str = "agent_1";
const DEFAULT_BUDGET_MS: u64 = 200;
const REPLAY_ITERATIONS: usize = 75;
const PROVIDER_TURNS: u64 = 250;

#[test]
fn perf_project_resume_plan_large_completed_log_under_budget() {
    // arrange
    // act
    // assert
    let events = completed_run_events(PROVIDER_TURNS);
    let budget = Duration::from_millis(
        std::env::var("HARNESS_PERF_RESUME_PLAN_BUDGET_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_BUDGET_MS),
    );

    let started = Instant::now();
    for _ in 0..REPLAY_ITERATIONS {
        let plan = project_resume_plan(events.iter(), RUN_ID).unwrap_or_abort();
        assert!(plan.is_resumable, "completed fixture remains resumable");
        assert_eq!(plan.max_seq, events.len() as u64);
    }
    let elapsed = started.elapsed();

    assert!(
        elapsed <= budget,
        "projecting {PROVIDER_TURNS} provider turns {REPLAY_ITERATIONS} times took {elapsed:?}, over budget {budget:?}"
    );
}

fn completed_run_events(provider_turns: u64) -> Vec<EventEnvelopeV1> {
    let mut seq = 1;
    let mut events = Vec::with_capacity(usize::try_from(provider_turns).unwrap_or(usize::MAX) + 3);
    events.push(envelope(
        seq,
        None,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".into(),
            workspace_root: "/tmp/harness-perf".to_string(),
        }),
    ));
    seq += 1;
    events.push(envelope(
        seq,
        Some(AGENT_ID),
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: AGENT_ID.to_string(),
            profile: "build".to_string(),
            parent_agent_id: None,
        }),
    ));
    seq += 1;

    for request in 1..=provider_turns {
        events.push(envelope(
            seq,
            Some(AGENT_ID),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: format!("req_{request}").into(),
                provider_id: "mock".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: format!("perf prompt {request}"),
                request_digest: format!("digest_{request}"),
                metadata: None,
            }),
        ));
        seq += 1;
    }

    events.push(envelope(
        seq,
        None,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    events
}

fn envelope(seq: u64, agent_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_{seq}"),
        seq,
        run_id: RUN_ID.into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(
            if agent_id.is_some() {
                ActorKind::Worker
            } else {
                ActorKind::System
            },
            agent_id.map(str::to_string),
        ),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{RUN_ID}")),
        payload,
    }
}
