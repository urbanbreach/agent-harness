use std::path::PathBuf;

use harness_core::context_budget::{BudgetStatus, RequestBudgetComponents, RequestBudgetSnapshot};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    ProviderRequestStartedMetadata, SCHEMA_VERSION,
};
use harness_core::UnwrapOrAbort;
use harness_providers::ProviderOutputCapDisposition;
use harness_tui::app::{AppState, LaunchMetadata};

use crate::model_switcher_fixtures::config_backed_profile_model_options;

fn context_budget_snapshot(
    status: BudgetStatus,
    occupied_input_tokens: u32,
    compaction_threshold_tokens: Option<u32>,
) -> RequestBudgetSnapshot {
    RequestBudgetSnapshot {
        status,
        requested_output_tokens: None,
        reserved_output_tokens: None,
        maximum_input_tokens: compaction_threshold_tokens
            .map(|threshold| threshold.saturating_add(8_192)),
        safety_margin_tokens: 0,
        compaction_threshold_tokens,
        components: RequestBudgetComponents {
            history_tokens: occupied_input_tokens,
            ..RequestBudgetComponents::default()
        },
        occupied_input_tokens,
        remaining_input_tokens: compaction_threshold_tokens
            .map(|threshold| threshold.saturating_sub(occupied_input_tokens)),
        requires_compaction: compaction_threshold_tokens
            .map(|threshold| occupied_input_tokens >= threshold),
        output_cap_disposition: ProviderOutputCapDisposition::UnspecifiedUnknownLimit,
    }
}

fn provider_started_with_budget(
    seq: u64,
    agent_id: &str,
    snapshot: RequestBudgetSnapshot,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-budget-{seq}"),
        seq,
        run_id: "run-budget".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
        correlation_id: Some(format!("req-budget-{seq}")),
        causation_id: None,
        stream_key: Some(format!("agent:{agent_id}")),
        payload: EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: format!("req-budget-{seq}").into(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "redacted".to_string(),
            request_digest: format!("digest-budget-{seq}"),
            metadata: Some(ProviderRequestStartedMetadata {
                context_budget: Some(snapshot),
                ..ProviderRequestStartedMetadata::default()
            }),
        }),
    }
}

#[test]
fn context_budget_snapshot_known_capacity_drives_status_segment() {
    // arrange: persisted launch metadata contains the shared estimated snapshot.
    let option = config_backed_profile_model_options("default")
        .into_iter()
        .find(|option| option.variant() == Some("deterministic"))
        .unwrap_or_abort();
    let snapshot = context_budget_snapshot(BudgetStatus::Estimated, 60_000, Some(119_808));
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::from_model_option(&option).with_last_request_budget(snapshot),
    );

    // act: the context-budget presentation is projected.
    let label = live.runtime_context_budget_text();

    // assert: the percentage denominator is the snapshot threshold, not the 128,000 maximum input.
    assert_eq!(label, Some("ctx ~60000/119808 50%".to_string()));
}

#[test]
fn context_budget_snapshot_unknown_capacity_avoids_percentage() {
    // arrange: replay metadata contains an unknown-capacity snapshot.
    let snapshot = context_budget_snapshot(BudgetStatus::UnknownLimits, 321, None);
    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-context-budget"), Vec::new());
    replay.set_launch_metadata(
        LaunchMetadata::new(
            "custom",
            "custom-provider",
            Some("unknown-model".to_string()),
        )
        .with_last_request_budget(snapshot),
    );

    // act: the context-budget presentation is projected.
    let label = replay.runtime_context_budget_text();

    // assert: unknown capacity is explicit and has no percentage.
    assert_eq!(label, Some("ctx ~321 · capacity unknown".to_string()));
    assert!(!label.unwrap_or_abort().contains('%'));
}

#[test]
fn context_budget_snapshot_unknown_missing_replay_metadata_is_explicit() {
    // arrange: an older replay has no persisted or event-scoped budget snapshot.
    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-old-budget"), Vec::new());
    replay.set_launch_metadata(LaunchMetadata::new(
        "legacy",
        "custom-provider",
        Some("legacy-model".to_string()),
    ));

    // act: the context-budget presentation is projected.
    let label = replay.runtime_context_budget_text();

    // assert: replay does not infer capacity from model registries.
    assert_eq!(label, Some("ctx ~0 · capacity unknown".to_string()));
}

#[test]
fn context_budget_snapshot_conservative_avoids_percentage() {
    // arrange: launch metadata contains a configured conservative snapshot.
    let snapshot = context_budget_snapshot(BudgetStatus::ConservativeFallback, 400, Some(500));
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::new("custom", "custom-provider", Some("model".to_string()))
            .with_last_request_budget(snapshot),
    );

    // act: the context-budget presentation is projected.
    let label = live.runtime_context_budget_text();

    // assert: conservative capacity is explicit and has no percentage.
    assert_eq!(label, Some("ctx ~400 · conservative".to_string()));
    assert!(!label.unwrap_or_abort().contains('%'));
}

#[test]
fn context_budget_snapshot_latest_request_sequence_wins_across_root_and_child() {
    // arrange: persisted metadata and root/child request snapshots disagree.
    let persisted = context_budget_snapshot(BudgetStatus::Estimated, 100, Some(1_000));
    let latest = context_budget_snapshot(BudgetStatus::Estimated, 700, Some(2_000));
    let stale = context_budget_snapshot(BudgetStatus::Estimated, 300, Some(1_500));
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::new("default", "default", Some("gpt-5.4-mini".to_string()))
            .with_last_request_budget(persisted),
    );

    // act: a newer child request arrives before a stale root event.
    live.ingest_event(provider_started_with_budget(20, "child", latest));
    live.ingest_event(provider_started_with_budget(10, "root", stale));

    // assert: event sequence, not ingestion order or actor kind, selects the snapshot.
    assert_eq!(
        live.runtime_context_budget_text(),
        Some("ctx ~700/2000 35%".to_string())
    );
}

#[test]
fn context_budget_snapshot_latest_request_without_snapshot_renders_unknown() {
    // arrange: a live request snapshot exists.
    let known = context_budget_snapshot(BudgetStatus::Estimated, 700, Some(2_000));
    let mut live = AppState::new_live(None, false, None);
    live.ingest_event(provider_started_with_budget(10, "root", known));
    let mut latest = provider_started_with_budget(20, "child", known);
    if let EventV1::ProviderRequestStarted(request) = &mut latest.payload {
        request.metadata = None;
    }

    // act: a newer request without migration metadata arrives.
    live.ingest_event(latest);

    // assert: the older capacity is not presented as current.
    assert_eq!(
        live.runtime_context_budget_text(),
        Some("ctx ~0 · capacity unknown".to_string())
    );
}

#[test]
fn context_budget_snapshot_replay_uses_request_event_before_persisted_bootstrap() {
    // arrange: replay launch metadata has an older persisted snapshot.
    let persisted = context_budget_snapshot(BudgetStatus::Estimated, 100, Some(1_000));
    let replayed = context_budget_snapshot(BudgetStatus::UnknownLimits, 777, None);
    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-budget-event"), Vec::new());
    replay.set_launch_metadata(
        LaunchMetadata::new("default", "default", Some("gpt-5.4-mini".to_string()))
            .with_last_request_budget(persisted),
    );

    // act: historical request-start evidence is replayed.
    replay.ingest_historical_event(provider_started_with_budget(42, "root", replayed));

    // assert: replay uses the event snapshot before the metadata bootstrap.
    assert_eq!(
        replay.runtime_context_budget_text(),
        Some("ctx ~777 · capacity unknown".to_string())
    );
}

#[test]
fn runtime_context_after_model_switch_keeps_snapshot_until_next_request() {
    // arrange: the active runtime has a request snapshot and another model is selectable.
    let snapshot = context_budget_snapshot(BudgetStatus::Estimated, 600, Some(1_200));
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(
        LaunchMetadata::new("default", "default", Some("current".to_string()))
            .with_last_request_budget(snapshot),
    );
    live.ingest_event(provider_started_with_budget(1, "root", snapshot));

    // act: launch metadata switches the next-turn model.
    live.set_launch_metadata(LaunchMetadata::new(
        "default",
        "default",
        Some("next".to_string()),
    ));

    // assert: current-runtime budget evidence remains active until another request starts.
    assert_eq!(
        live.runtime_context_budget_text(),
        Some("ctx ~600/1200 50%".to_string())
    );
}

#[test]
fn context_budget_snapshot_refreshing_precedes_latest_snapshot() {
    // arrange: the latest request has an estimated snapshot.
    let snapshot = context_budget_snapshot(BudgetStatus::Estimated, 600, Some(1_200));
    let mut live = AppState::new_live(None, false, None);
    live.ingest_event(provider_started_with_budget(1, "root", snapshot));

    // act: compaction applies without a refreshed token estimate.
    live.ingest_event(EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-compaction".to_string(),
        seq: 2,
        run_id: "run-budget".into(),
        mono_ms: 2,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run-budget".to_string()),
        payload: serde_json::from_value(serde_json::json!({
            "event_type": "compaction_applied",
            "data": {
                "checkpoint_id": "checkpoint-1",
                "agent_id": "root",
                "through_seq": 1,
                "through_request_id": "req-budget-1",
                "tokens_before_estimate": 600,
                "tokens_after_estimate": null,
                "summary_tokens_estimate": null,
                "compacted_turns": 1,
                "preserved_turns": 1,
                "reduction_tokens_estimate": null,
                "reduction_percent_estimate": null,
                "estimate_source": null
            }
        }))
        .unwrap_or_abort(),
    });

    // assert: refresh precedence hides the stale request snapshot.
    assert_eq!(
        live.runtime_context_budget_text(),
        Some("ctx compacted · refreshing".to_string())
    );
}
