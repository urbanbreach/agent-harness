pub(super) const AGENT_ID: &str = "agent_000001";

pub(super) struct CompactionFixture<'a> {
    pub(super) seq: u64,
    pub(super) first_kept_event_seq: u64,
    pub(super) first_kept_entry_id: &'a EntryId,
    pub(super) summary: &'a str,
}

pub(super) fn legacy_user_entry_id(seq: u64) -> EntryId {
    EventIdentityNamespace::new(&RunId::new("run_conversation_projection")).entry_id(
        seq,
        &format!("evt-{seq:020}"),
        "user_message",
    )
}

pub(super) fn compaction_event(fixture: CompactionFixture<'_>) -> EventEnvelopeV1 {
    let payload = serde_json::from_value(json!({
        "event_type": "session_compaction",
        "data": {
            "agent_id": AGENT_ID,
            "summary": fixture.summary,
            "first_kept_event_seq": fixture.first_kept_event_seq,
            "first_kept_entry_id": fixture.first_kept_entry_id,
            "tokens_before": 100,
            "tokens_after": 37,
            "summary_generation_usage": {
                "prompt_tokens": 11,
                "completion_tokens": 5,
                "total_tokens": 16
            },
            "trigger_reason": "test",
            "from_hook": false
        }
    }))
    .unwrap_or_abort();
    envelope(fixture.seq, worker(), None, payload)
}

pub(super) fn user_event(seq: u64, request_id: &str, text: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        worker(),
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: text.to_string(),
        }),
    )
}
