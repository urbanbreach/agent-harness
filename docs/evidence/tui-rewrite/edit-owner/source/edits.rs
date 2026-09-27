use super::*;
use harness_core::conversation_rewind::ConversationRewoundEvent;
use harness_core::event::{
    EditAppliedEvent, EditProposedEvent, EditRejectedEvent, ToolCallRequestedEvent,
};

#[test]
fn rewinding_later_edit_results_restores_the_retained_tool_proposal() {
    // A later turn can finish an earlier tool. Rewind must remove that result
    // while retaining the tool and its original proposal, just like replay.
    for result in [
        EventV1::EditApplied(EditAppliedEvent {
            edit_id: "edit".into(),
            path: "demo.txt".into(),
            new_file_digest: "new-file".into(),
            diff_rel_path: None,
            diff_digest: None,
        }),
        EventV1::EditRejected(EditRejectedEvent {
            edit_id: "edit".into(),
            path: "demo.txt".into(),
            reason: "discarded edit result".into(),
        }),
    ] {
        let result_marker = match &result {
            EventV1::EditApplied(_) => "Patch demo.txt",
            _ => "discarded edit result",
        };
        let mut history = vec![
            durable_envelope(
                1,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "turn-1".into(),
                    text: "first prompt".into(),
                }),
            ),
            durable_envelope(
                2,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "provider-1".into(),
                    provider_id: "mock".into(),
                    model_id: "model".into(),
                    prompt_summary: "first prompt".into(),
                    request_digest: "request".into(),
                    metadata: None,
                }),
            ),
            durable_envelope(
                3,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "tool-1".into(),
                    tool_id: "edit.hashline_apply".into(),
                    args_summary: r#"{"path":"demo.txt"}"#.into(),
                    args_digest: "args".into(),
                    metadata: None,
                }),
            ),
        ];
        history[0].actor = EventActor::new(ActorKind::User, None);
        let mut proposal = durable_envelope(
            4,
            EventV1::EditProposed(EditProposedEvent {
                edit_id: "edit".into(),
                path: "demo.txt".into(),
                summary: "retained proposal".into(),
                patch_digest: "patch".into(),
            }),
        );
        proposal.correlation_id = Some("tool-1".into());
        history.push(proposal);
        let mut app = AppState::new_live(None, false, None);
        app.set_reduced_motion_for_evidence(true);
        for event in &history {
            app.ingest_event(event.clone());
        }
        app.set_tool_output_expanded_for_test("tool-1", true);
        let before = render(&app);
        assert!(before.contains("Edit demo.txt"), "{before}");

        history.push(durable_envelope(
            5,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-2".into(),
                text: "second prompt".into(),
            }),
        ));
        history[4].actor = EventActor::new(ActorKind::User, None);
        history[4].correlation_id = Some("turn-2".into());
        let mut finish = durable_envelope(6, result);
        finish.correlation_id = Some("tool-1".into());
        history.push(finish);
        for event in &history[4..] {
            app.ingest_event(event.clone());
        }
        let changed = render(&app);
        assert!(changed.contains("second prompt"), "{changed}");
        assert!(changed.contains(result_marker), "{changed}");

        assert!(harness_core::conversation_rewind::rewind_points(&history)
            .iter()
            .any(|point| point.seq == 5 && point.request_id == "turn-2"));
        let rewind = durable_envelope(
            7,
            EventV1::ConversationRewound(ConversationRewoundEvent {
                target_seq: 5,
                request_id: "turn-2".into(),
            }),
        );
        history.push(rewind.clone());
        app.ingest_event(rewind);
        let rewound = render(&app);
        assert!(rewound.contains("Edit demo.txt"), "{rewound}");
        assert!(!rewound.contains("Patch demo.txt"), "{rewound}");
        assert!(!rewound.contains("discarded edit result"), "{rewound}");
        assert!(!rewound.contains("second prompt"), "{rewound}");

        // The retained history remains authoritative when opened afresh.
        let mut reopened = AppState::new_live(None, false, None);
        reopened.set_reduced_motion_for_evidence(true);
        reopened.replace_events(history);
        let replayed = render(&reopened);
        assert!(replayed.contains("Edit demo.txt"), "{replayed}");
        assert!(!replayed.contains("Patch demo.txt"), "{replayed}");
        assert!(!replayed.contains("discarded edit result"), "{replayed}");
    }
}
