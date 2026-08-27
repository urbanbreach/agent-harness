#[test]
fn compaction_v2_old_branch_summary_not_reintroduced() {
    // arrange
    // act
    // assert
    let first_kept_entry_id = legacy_user_entry_id(3);
    let events = vec![
        user_event(1, "old", "old turn"),
        envelope(
            2,
            worker(),
            None,
            EventV1::BranchSummary(BranchSummaryEvent {
                agent_id: AGENT_ID.to_string(),
                summary: "abandoned branch".to_string(),
                from_event_seq: 1,
                read_files: Vec::new(),
                modified_files: Vec::new(),
                from_hook: false,
            }),
        ),
        user_event(3, "kept", "kept turn"),
        compaction_event(CompactionFixture {
            seq: 4,
            first_kept_event_seq: 3,
            first_kept_entry_id: &first_kept_entry_id,
            summary: "active summary",
        }),
    ];

    let projected_text = project_conversation(&events, &[])
        .unwrap_or_abort()
        .messages
        .into_iter()
        .filter_map(|message| match message {
            ConversationMessage::Checkpoint(checkpoint) => Some(checkpoint.summary),
            ConversationMessage::User(user) => Some(user.text),
            ConversationMessage::Assistant(_) | ConversationMessage::ToolResult(_) => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        projected_text,
        vec!["active summary".to_string(), "kept turn".to_string()]
    );
}

#[test]
fn compaction_v2_first_kept_entry_id_is_the_typed_boundary() {
    // arrange
    // act
    // assert
    let typed_boundary = legacy_user_entry_id(3);
    let events = vec![
        user_event(1, "legacy-cut", "must be summarized"),
        user_event(3, "typed-cut", "typed suffix"),
        compaction_event(CompactionFixture {
            seq: 4,
            first_kept_event_seq: 1,
            first_kept_entry_id: &typed_boundary,
            summary: "active summary",
        }),
    ];

    let projected_text = project_conversation(&events, &[])
        .unwrap_or_abort()
        .messages
        .into_iter()
        .filter_map(|message| match message {
            ConversationMessage::Checkpoint(checkpoint) => Some(checkpoint.summary),
            ConversationMessage::User(user) => Some(user.text),
            ConversationMessage::Assistant(_) | ConversationMessage::ToolResult(_) => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        projected_text,
        vec!["active summary".to_string(), "typed suffix".to_string()]
    );
}

#[test]
fn compaction_v2_legacy_sequence_boundary_remains_read_only_fallback() {
    // arrange
    // act
    // assert
    let payload = serde_json::from_value(json!({
        "event_type": "session_compaction",
        "data": {
            "agent_id": AGENT_ID,
            "summary": "legacy summary",
            "first_kept_event_seq": 3,
            "tokens_before": 100,
            "trigger_reason": "legacy",
            "from_hook": false
        }
    }))
    .unwrap_or_abort();
    let events = vec![
        user_event(1, "old", "legacy old"),
        user_event(3, "kept", "legacy kept"),
        envelope(4, worker(), None, payload),
    ];

    let projected_text = project_conversation(&events, &[])
        .unwrap_or_abort()
        .messages
        .into_iter()
        .filter_map(|message| match message {
            ConversationMessage::Checkpoint(checkpoint) => Some(checkpoint.summary),
            ConversationMessage::User(user) => Some(user.text),
            ConversationMessage::Assistant(_) | ConversationMessage::ToolResult(_) => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        projected_text,
        vec!["legacy summary".to_string(), "legacy kept".to_string()]
    );
}

#[test]
fn compaction_v2_unknown_typed_boundary_preserves_prior_valid_boundary() {
    // arrange
    // act
    // assert
    let valid_boundary = legacy_user_entry_id(3);
    let unknown_boundary = EntryId::new("entry-not-present-on-active-path");
    let events = vec![
        user_event(1, "old", "summarized by valid boundary"),
        user_event(3, "kept", "retained suffix"),
        compaction_event(CompactionFixture {
            seq: 4,
            first_kept_event_seq: 3,
            first_kept_entry_id: &valid_boundary,
            summary: "last valid summary",
        }),
        compaction_event(CompactionFixture {
            seq: 5,
            first_kept_event_seq: 5,
            first_kept_entry_id: &unknown_boundary,
            summary: "malformed summary must be ignored",
        }),
    ];

    let projected_text = project_conversation(&events, &[])
        .unwrap_or_abort()
        .messages
        .into_iter()
        .filter_map(|message| match message {
            ConversationMessage::Checkpoint(checkpoint) => Some(checkpoint.summary),
            ConversationMessage::User(user) => Some(user.text),
            ConversationMessage::Assistant(_) | ConversationMessage::ToolResult(_) => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        projected_text,
        vec!["last valid summary".to_string(), "retained suffix".to_string()]
    );
}

#[test]
fn compaction_v2_orphan_tool_result_excluded() {
    // arrange
    // act
    // assert
    let events = vec![envelope(
        1,
        worker(),
        Some("orphan-request"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: ToolCallId::new("missing-call"),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("orphan output".to_string()),
            output_digest: Some("digest".to_string()),
            output_json: None,
            metadata: None,
        }),
    )];

    let projection = project_conversation(&events, &[]).unwrap_or_abort();

    assert!(projection.messages.is_empty());
}

#[test]
fn compaction_v2_malformed_trailing_event_has_no_side_effects() {
    // arrange
    // act
    // assert
    let first_kept_entry_id = legacy_user_entry_id(1);
    let mut events = vec![
        user_event(1, "kept", "kept turn"),
        compaction_event(CompactionFixture {
            seq: 2,
            first_kept_event_seq: 1,
            first_kept_entry_id: &first_kept_entry_id,
            summary: "active summary",
        }),
    ];
    events.push(envelope(
        3,
        worker(),
        Some("bad"),
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "unknown-provider".into(),
            delta: "dangling fragment".to_string(),
        }),
    ));
    let unchanged = events.clone();

    let error = project_conversation(&events, &[]).unwrap_err();

    assert_eq!(
        (error, events),
        (
            ConversationProjectionError::ProviderDeltaBeforeStart {
                request_id: "unknown-provider".to_string(),
                seq: 3,
            },
            unchanged,
        )
    );
}
