use super::*;

use super::ui_subagent_footer_exact_tests::{footer_event, render_footer_rows};

#[test]
fn subagent_footer_body_sticks_to_latest_activity() {
    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("agent_child".to_string()),
    );
    let transcript = (1..=24)
        .map(|index| format!("child activity line {index:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-footer/agent_child"),
        vec![
            footer_event(
                1,
                worker.clone(),
                Some("req_child"),
                harness_core::event::EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "req_child".to_string(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "audit transcript parity".to_string(),
                        request_digest: "digest-child-start".to_string(),
                        metadata: None,
                    },
                ),
            ),
            footer_event(
                2,
                worker,
                Some("req_child"),
                harness_core::event::EventV1::ProviderStreamDelta(
                    harness_core::event::ProviderStreamDeltaEvent {
                        request_id: "req_child".to_string(),
                        delta: transcript,
                    },
                ),
            ),
        ],
    );

    let rendered = render_footer_rows(&app).join("\n");
    assert!(
        rendered.contains("child activity line 24"),
        "subagent footer should stick to the newest child activity like the reference sticky-bottom scrollbox\n{rendered}"
    );
    assert!(
        !rendered.contains("child activity line 01"),
        "overflowing subagent footer should not open at the oldest child activity\n{rendered}"
    );
}

#[test]
fn subagent_footer_body_renders_child_user_commit_without_assistant_summary() {
    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("agent_child".to_string()),
    );
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-footer/agent_child"),
        vec![
            footer_event(
                1,
                harness_core::event::EventActor::new(harness_core::event::ActorKind::User, None),
                Some("req_child"),
                harness_core::event::EventV1::UserMessageSubmitted(
                    harness_core::event::UserMessageSubmittedEvent {
                        request_id: "req_child".to_string(),
                        text: "Inspect footer parity".to_string(),
                    },
                ),
            ),
            footer_event(
                2,
                worker.clone(),
                Some("req_child"),
                harness_core::event::EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "req_child".to_string(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "Inspect footer parity".to_string(),
                        request_digest: "digest-child-start".to_string(),
                        metadata: None,
                    },
                ),
            ),
            footer_event(
                3,
                worker,
                Some("req_child"),
                harness_core::event::EventV1::ProviderStreamDelta(
                    harness_core::event::ProviderStreamDeltaEvent {
                        request_id: "req_child".to_string(),
                        delta: "child body line".to_string(),
                    },
                ),
            ),
        ],
    );

    let rendered = render_footer_rows(&app).join("\n");
    assert!(
        rendered.contains("child body line"),
        "subagent footer should render child assistant body\n{rendered}"
    );
    assert!(
        rendered.contains("› Inspect footer parity"),
        "reference footer renders child user commits with prompt-marker text\n{rendered}"
    );
    assert!(
        !rendered.contains("Assistant · model-1"),
        "reference entry body suppresses summary commits; footer body should not spend a row on Harness assistant footer chrome\n{rendered}"
    );
}

#[test]
fn subagent_footer_body_preserves_child_text_matching_parent_label() {
    let worker = harness_core::event::EventActor::new(
        harness_core::event::ActorKind::Worker,
        Some("agent_child".to_string()),
    );
    let app = AppState::new_replay(
        std::path::PathBuf::from("/tmp/harness-subagent-footer/agent_child"),
        vec![
            footer_event(
                1,
                worker.clone(),
                Some("req_child"),
                harness_core::event::EventV1::ProviderRequestStarted(
                    harness_core::event::ProviderRequestStartedEvent {
                        request_id: "req_child".to_string(),
                        provider_id: "mock".to_string(),
                        model_id: "model-1".to_string(),
                        prompt_summary: "audit transcript parity".to_string(),
                        request_digest: "digest-child-start".to_string(),
                        metadata: None,
                    },
                ),
            ),
            footer_event(
                2,
                worker,
                Some("req_child"),
                harness_core::event::EventV1::ProviderStreamDelta(
                    harness_core::event::ProviderStreamDeltaEvent {
                        request_id: "req_child".to_string(),
                        delta: "child output mentions parent_run literally".to_string(),
                    },
                ),
            ),
        ],
    );

    let rendered = render_footer_rows(&app).join("\n");
    assert!(
        rendered.contains("parent_run literally"),
        "subagent footer must not mutate child output that happens to contain the parent label\n{rendered}"
    );
}
