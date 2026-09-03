use super::*;

fn command_group_app(command_count: usize) -> (AppState, Vec<String>) {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(provider_started(
        1,
        "req_tool_group_parity",
        "default",
        "gpt-5.4-mini",
    ));
    let mut tool_call_ids = Vec::with_capacity(command_count);
    for index in 0..command_count {
        let tool_call_id = format!("tc_group_parity_{index:02}");
        let seq = u64::try_from(index).unwrap_or_abort() * 2 + 2;
        app.ingest_event(envelope(
            seq,
            "req_tool_group_parity",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tool_call_id.clone().into(),
                tool_id: "bash".to_string(),
                args_summary: format!(r#"{{"command":"printf command-{index:02}"}}"#),
                args_digest: format!("digest-group-parity-{index:02}-args"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq + 1,
            "req_tool_group_parity",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tool_call_id.clone().into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(format!("command-{index:02}")),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));
        tool_call_ids.push(tool_call_id);
    }
    (app, tool_call_ids)
}

pub(super) fn zero_tool_groups_render_without_fold_affordance() {
    // arrange
    let (app, _) = command_group_app(0);

    // act
    let screen = render_text(&app, 140, 40);

    // assert
    assert!(!screen.contains("Ran "), "{screen}");
    assert!(!screen.contains(" more"), "{screen}");
}

pub(super) fn one_tool_group_member_renders_once_without_more_affordance() {
    // arrange
    let (app, _) = command_group_app(1);

    // act
    let screen = render_text(&app, 140, 40);

    // assert
    assert_eq!(screen.matches("printf command-00").count(), 1, "{screen}");
    assert!(!screen.contains("1 more"), "{screen}");
}

pub(super) fn many_tool_group_members_render_one_exact_hidden_count_affordance() {
    // arrange
    let (app, _) = command_group_app(14);

    // act
    let screen = render_text(&app, 140, 40);

    // assert
    assert_eq!(screen.matches("4 more").count(), 1, "{screen}");
    assert!(screen.contains("Ran 14 commands"), "{screen}");
}

pub(super) fn tool_group_fold_round_trip_survives_compaction_and_narrow_reflow() {
    // arrange
    let (mut app, tool_call_ids) = command_group_app(14);
    let target = TranscriptMouseTarget::ToolGroup {
        tool_call_ids: tool_call_ids.clone(),
    };
    assert_eq!(render_text(&app, 140, 40).matches("4 more").count(), 1);

    // act
    app.activate_transcript_mouse_target(target.clone());
    let expanded_count = app
        .transcript_view
        .expanded_tool_outputs
        .intersection(&tool_call_ids.iter().cloned().collect())
        .count();
    app.activate_transcript_mouse_target(target.clone());
    app.ingest_event(envelope(
        31,
        "compaction:agent_tool_group_parity",
        EventV1::CompactionWritten(CompactionWrittenEvent {
            checkpoint_id: "checkpoint_tool_group_parity".to_string(),
            agent_id: "agent_tool_group_parity".to_string(),
            artifact_path: "artifacts/compactions/agent_tool_group_parity/checkpoint.json"
                .to_string(),
            artifact_digest: Some("digest-tool-group-compaction".to_string()),
            artifact_bytes: 64,
            trigger_reason: "manual".to_string(),
            through_seq: 30,
            through_request_id: Some("req_tool_group_parity".to_string()),
            provider_id: Some("default".to_string()),
            model_id: Some("gpt-5.4-mini".to_string()),
            tokens_before: Some(1_000),
            tokens_before_estimate: Some(1_000),
            tokens_after_estimate: Some(400),
            summary_tokens_estimate: Some(80),
            compacted_turns: Some(1),
            reduction_tokens_estimate: Some(600),
            reduction_percent_estimate: Some(60),
            estimate_source: Some("fixture".to_string()),
            summary_source: None,
            preserved_turns: 1,
        }),
    ));
    let narrow_screen = render_text(&app, 80, 50);

    // assert
    assert_eq!(expanded_count, 14);
    assert!(app
        .transcript_view
        .expanded_tool_outputs
        .is_disjoint(&tool_call_ids.iter().cloned().collect()));
    assert_eq!(
        app.transcript_view
            .collapsed_tool_outputs
            .intersection(&tool_call_ids.iter().cloned().collect())
            .count(),
        14
    );
    assert_eq!(
        narrow_screen.matches("4 more").count(),
        1,
        "{narrow_screen}"
    );

    // act
    app.activate_transcript_mouse_target(target);

    // assert
    assert_eq!(
        app.transcript_view
            .expanded_tool_outputs
            .intersection(&tool_call_ids.into_iter().collect())
            .count(),
        14
    );
}
