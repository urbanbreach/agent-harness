use super::*;
use crate::config::CompactionSettings;
use crate::conversation::{
    ConversationAssistantMessage, ConversationCheckpointMessage, ConversationMessage,
    ConversationToolCall, ConversationToolResultMessage, ConversationUserMessage,
};
use crate::event::{
    ActorKind, AssistantMessageFinishedEvent, BranchSummaryEvent, EventActor, EventEnvelopeV1,
    EventV1, ProviderRequestStartedEvent, ProviderStreamDeltaEvent, SessionCompactionEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use crate::ids::{RequestId, ToolCallId};

// =========================================================================
// Test helpers
// =========================================================================

fn envelope(seq: u64, agent_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_test".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn user_msg(seq: u64, agent_id: &str, request_id: &str, text: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: RequestId::new(request_id),
            text: text.to_string(),
        }),
    )
}

fn stream_delta(seq: u64, agent_id: &str, request_id: &str, delta: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: RequestId::new(request_id),
            delta: delta.to_string(),
        }),
    )
}

fn assistant_finished(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: RequestId::new(request_id),
            tool_call_count: 0,
            assistant_message: None,
        }),
    )
}

fn tool_requested(seq: u64, agent_id: &str, tool_id: &str, args_summary: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: ToolCallId::new(&format!("tc-{seq}")),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{seq}"),
            metadata: None,
        }),
    )
}

fn tool_finished(seq: u64, agent_id: &str, output_summary: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: ToolCallId::new(&format!("tc-{seq}")),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(output_summary.to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    )
}

fn settings(enabled: bool, reserve_tokens: u32, keep_recent_tokens: u32) -> CompactionSettings {
    CompactionSettings {
        enabled,
        reserve_tokens,
        keep_recent_tokens,
        ..Default::default()
    }
}

// =========================================================================
// calculate_context_tokens
// =========================================================================

#[test]
fn calculate_context_tokens_sums_all_components() {
    assert_eq!(calculate_context_tokens(1000, 500, 200, 100), 1800);
}

#[test]
fn calculate_context_tokens_handles_zeros() {
    assert_eq!(calculate_context_tokens(0, 0, 0, 0), 0);
}

#[test]
fn calculate_context_tokens_saturates_on_overflow() {
    assert_eq!(
        calculate_context_tokens(u32::MAX, u32::MAX, u32::MAX, u32::MAX),
        u32::MAX
    );
}

// =========================================================================
// estimate_text_tokens
// =========================================================================

#[test]
fn estimate_text_tokens_empty_returns_zero() {
    assert_eq!(estimate_text_tokens(""), 0);
}

#[test]
fn estimate_text_tokens_four_bytes_one_token() {
    assert_eq!(estimate_text_tokens("abcd"), 1);
}

#[test]
fn estimate_text_tokens_five_bytes_two_tokens() {
    assert_eq!(estimate_text_tokens("abcde"), 2);
}

#[test]
fn estimate_text_tokens_uses_byte_length_not_char_count() {
    // Multi-byte UTF-8: each of these chars is 3 bytes.
    // 4 chars * 3 bytes = 12 bytes -> ceil(12/4) = 3 tokens.
    assert_eq!(estimate_text_tokens("\u{3042}\u{3044}\u{3046}\u{3048}"), 3);
}

// =========================================================================
// estimate_message_tokens
// =========================================================================

#[test]
fn estimate_message_tokens_user_message() {
    let msg = ConversationMessage::User(ConversationUserMessage {
        request_id: RequestId::new("req-1"),
        text: "abcdefgh".to_string(),
        seq: None,
        agent_id: None,
    });
    assert_eq!(estimate_message_tokens(&msg), 2);
}

#[test]
fn estimate_message_tokens_assistant_with_text_only() {
    let msg = ConversationMessage::Assistant(ConversationAssistantMessage {
        request_id: RequestId::new("req-1"),
        agent_id: None,
        text: "abcdefgh".to_string(),
        tool_calls: vec![],
        stop_reason: None,
        first_seq: None,
        last_seq: None,
        provider_id: None,
        model_id: None,
        output_digest: None,
    });
    assert_eq!(estimate_message_tokens(&msg), 2);
}

#[test]
fn estimate_message_tokens_assistant_with_tool_calls() {
    let msg = ConversationMessage::Assistant(ConversationAssistantMessage {
        request_id: RequestId::new("req-1"),
        agent_id: None,
        text: "abcd".to_string(),
        tool_calls: vec![ConversationToolCall {
            tool_call_id: ToolCallId::new("tc-1"),
            tool_id: "read".to_string(),
            args_summary: "{\"path\":\"/foo\"}".to_string(),
            args_digest: "digest".to_string(),
            seq: None,
            metadata: None,
        }],
        stop_reason: None,
        first_seq: None,
        last_seq: None,
        provider_id: None,
        model_id: None,
        output_digest: None,
    });
    // text: 4 bytes, tool_id: 4 bytes, args_summary: 15 bytes -> total 23 -> ceil(23/4) = 6
    assert_eq!(estimate_message_tokens(&msg), 6);
}

#[test]
fn estimate_message_tokens_tool_result_with_summary() {
    let msg = ConversationMessage::ToolResult(Box::new(ConversationToolResultMessage {
        request_id: RequestId::new("req-1"),
        tool_call_id: ToolCallId::new("tc-1"),
        tool_id: Some("read".to_string()),
        status: ToolCallStatus::Succeeded,
        output_summary: Some("abcdefgh".to_string()),
        output_digest: None,
        output_json: None,
        seq: None,
        metadata: None,
    }));
    assert_eq!(estimate_message_tokens(&msg), 2);
}

#[test]
fn estimate_message_tokens_checkpoint() {
    let msg = ConversationMessage::Checkpoint(ConversationCheckpointMessage {
        checkpoint_id: "cp-1".to_string(),
        agent_id: "agent-1".to_string(),
        through_seq: 10,
        summary: "abcdefgh".to_string(),
    });
    assert_eq!(estimate_message_tokens(&msg), 2);
}

// =========================================================================
// estimate_messages_tokens
// =========================================================================

#[test]
fn estimate_messages_tokens_sums_all_messages() {
    let messages = vec![
        ConversationMessage::User(ConversationUserMessage {
            request_id: RequestId::new("req-1"),
            text: "abcd".to_string(),
            seq: None,
            agent_id: None,
        }),
        ConversationMessage::Assistant(ConversationAssistantMessage {
            request_id: RequestId::new("req-1"),
            agent_id: None,
            text: "efgh".to_string(),
            tool_calls: vec![],
            stop_reason: None,
            first_seq: None,
            last_seq: None,
            provider_id: None,
            model_id: None,
            output_digest: None,
        }),
    ];
    // 4 bytes + 4 bytes = 8 -> ceil(8/4) = 2
    assert_eq!(estimate_messages_tokens(&messages), 2);
}

// =========================================================================
// estimate_context_tokens
// =========================================================================

#[test]
fn estimate_context_tokens_returns_estimated_when_no_usage_anchor() {
    let messages = vec![
        ConversationMessage::User(ConversationUserMessage {
            request_id: RequestId::new("req-1"),
            text: "abcd".to_string(),
            seq: None,
            agent_id: None,
        }),
        ConversationMessage::Assistant(ConversationAssistantMessage {
            request_id: RequestId::new("req-1"),
            agent_id: None,
            text: "efgh".to_string(),
            tool_calls: vec![],
            stop_reason: None,
            first_seq: None,
            last_seq: None,
            provider_id: None,
            model_id: None,
            output_digest: None,
        }),
    ];

    let estimate = estimate_context_tokens(&messages);

    assert!(estimate.estimated);
    assert_eq!(estimate.last_assistant_usage, None);
    assert_eq!(estimate.total_tokens, 2);
}

#[test]
fn estimate_context_tokens_empty_messages() {
    let estimate = estimate_context_tokens(&[]);
    assert!(estimate.estimated);
    assert_eq!(estimate.total_tokens, 0);
    assert_eq!(estimate.last_assistant_usage, None);
}

// =========================================================================
// should_compact
// =========================================================================

#[test]
fn should_compact_true_when_context_exceeds_threshold() {
    let s = settings(true, 10000, 20000);
    assert!(should_compact(95000, 100000, &s));
}

#[test]
fn should_compact_false_when_below_threshold() {
    let s = settings(true, 10000, 20000);
    assert!(!should_compact(89000, 100000, &s));
}

#[test]
fn should_compact_false_when_disabled() {
    let s = settings(false, 10000, 20000);
    assert!(!should_compact(95000, 100000, &s));
}

#[test]
fn should_compact_boundary_exact_threshold() {
    // context_tokens > context_window - reserve_tokens
    // 90000 > 100000 - 10000 = 90000 -> false (not strictly greater)
    let s = settings(true, 10000, 20000);
    assert!(!should_compact(90000, 100000, &s));
}

#[test]
fn should_compact_boundary_one_above_threshold() {
    // 90001 > 90000 -> true
    let s = settings(true, 10000, 20000);
    assert!(should_compact(90001, 100000, &s));
}

#[test]
fn should_compact_zero_context_window_with_tokens() {
    let s = settings(true, 10000, 20000);
    // 0u32.saturating_sub(10000) = 0, so context_tokens > 0
    assert!(should_compact(1, 0, &s));
}

#[test]
fn should_compact_zero_context_window_zero_tokens() {
    let s = settings(true, 10000, 20000);
    assert!(!should_compact(0, 0, &s));
}

// =========================================================================
// find_cut_point
// =========================================================================

#[test]
fn find_cut_point_returns_none_for_no_agent_events() {
    let events = vec![user_msg(1, "other_agent", "req-1", "hello")];
    assert!(find_cut_point(&events, "agent-1", 1000).is_none());
}

#[test]
fn find_cut_point_keeps_recent_turn_if_budget_fits() {
    let events = vec![
        user_msg(1, "agent-1", "req-1", "hello"),
        stream_delta(2, "agent-1", "req-1", "world"),
        assistant_finished(3, "agent-1", "req-1"),
        user_msg(4, "agent-1", "req-2", "again"),
        stream_delta(5, "agent-1", "req-2", "resp"),
        assistant_finished(6, "agent-1", "req-2"),
    ];

    let result = find_cut_point(&events, "agent-1", 50000).unwrap();

    assert_eq!(result.first_kept_event_seq, 4);
    assert!(!result.is_split_turn);
    assert_eq!(result.turn_start_seq, None);
    assert!(result.tokens_before > 0);
}

#[test]
fn find_cut_point_never_cuts_at_tool_result() {
    // Build a turn: user -> assistant (with tool call) -> tool result -> assistant finished
    let events = vec![
        user_msg(1, "agent-1", "req-1", "short"),
        stream_delta(2, "agent-1", "req-1", &"x".repeat(100)),
        tool_requested(3, "agent-1", "read", "read file"),
        tool_finished(4, "agent-1", &"x".repeat(200)),
        assistant_finished(5, "agent-1", "req-1"),
        user_msg(6, "agent-1", "req-2", "next"),
        stream_delta(7, "agent-1", "req-2", &"y".repeat(50)),
        assistant_finished(8, "agent-1", "req-2"),
    ];

    // Small budget to force a cut
    let result = find_cut_point(&events, "agent-1", 30).unwrap();

    // The cut point must be a UserMessageSubmitted or AssistantMessageFinished,
    // never a ToolCallFinished.
    let cut_event = events
        .iter()
        .find(|e| e.seq == result.first_kept_event_seq)
        .unwrap();
    assert!(
        matches!(
            cut_event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_)
        ),
        "cut point must be user or assistant event, got seq {}",
        result.first_kept_event_seq
    );
}

#[test]
fn find_cut_point_detects_split_turn() {
    // Turn 1: user -> assistant
    // Turn 2: user -> assistant A2-1 -> assistant A2-2 -> assistant A2-3
    let events = vec![
        user_msg(1, "agent-1", "req-1", "Turn 1"),
        stream_delta(2, "agent-1", "req-1", &"A".repeat(100)),
        assistant_finished(3, "agent-1", "req-1"),
        user_msg(4, "agent-1", "req-2", "Turn 2"),
        stream_delta(5, "agent-1", "req-2", &"B".repeat(100)),
        assistant_finished(6, "agent-1", "req-2"),
        stream_delta(7, "agent-1", "req-2", &"C".repeat(100)),
        assistant_finished(8, "agent-1", "req-2"),
        stream_delta(9, "agent-1", "req-2", &"D".repeat(100)),
        assistant_finished(10, "agent-1", "req-2"),
    ];

    // With keep_recent_tokens = 75, should cut somewhere in Turn 2
    let result = find_cut_point(&events, "agent-1", 75).unwrap();

    let cut_event = events
        .iter()
        .find(|e| e.seq == result.first_kept_event_seq)
        .unwrap();

    if matches!(cut_event.payload, EventV1::AssistantMessageFinished(_)) {
        assert!(result.is_split_turn);
        assert_eq!(result.turn_start_seq, Some(4));
    }
}

#[test]
fn find_cut_point_cuts_at_user_message_when_budget_exceeds() {
    let events = vec![
        user_msg(1, "agent-1", "req-1", &"x".repeat(100)),
        stream_delta(2, "agent-1", "req-1", &"y".repeat(100)),
        assistant_finished(3, "agent-1", "req-1"),
        user_msg(4, "agent-1", "req-2", &"z".repeat(100)),
        stream_delta(5, "agent-1", "req-2", &"w".repeat(100)),
        assistant_finished(6, "agent-1", "req-2"),
    ];

    // With keep_recent_tokens = 50, walking backward:
    //   i=5 (assistant_finished, 0 tokens) -> skip
    //   i=4 (stream_delta, 25 tokens) -> accumulated=25 (< 50)
    //   i=3 (user_msg, 25 tokens) -> accumulated=50 (>= 50, stop)
    // Nearest valid cut point at or after index 3 is index 3 (user_msg, seq=4).
    let result = find_cut_point(&events, "agent-1", 50).unwrap();

    assert_eq!(result.first_kept_event_seq, 4);
    assert!(!result.is_split_turn);
    assert_eq!(result.turn_start_seq, None);
    assert_eq!(result.first_kept_request_id.as_deref(), Some("req-2"));
}

#[test]
fn find_cut_point_single_message_keeps_latest_turn() {
    let events = vec![
        user_msg(1, "agent-1", "req-1", "hello"),
        assistant_finished(2, "agent-1", "req-1"),
    ];

    let result = find_cut_point(&events, "agent-1", 1000).unwrap();
    assert_eq!(result.first_kept_event_seq, 1);
    assert!(!result.is_split_turn);
}

#[test]
fn find_cut_point_tokens_before_is_total() {
    let events = vec![
        user_msg(1, "agent-1", "req-1", "abcd"),
        stream_delta(2, "agent-1", "req-1", "efgh"),
        assistant_finished(3, "agent-1", "req-1"),
    ];

    let result = find_cut_point(&events, "agent-1", 1000).unwrap();
    // 4 bytes (user) + 4 bytes (delta) = 8 bytes -> ceil(8/4) = 2 tokens
    assert_eq!(result.tokens_before, 2);
}

#[test]
fn find_cut_point_filters_by_agent_id() {
    let events = vec![
        user_msg(1, "agent-1", "req-1", "hello"),
        assistant_finished(2, "agent-1", "req-1"),
        user_msg(3, "agent-2", "req-2", "other agent"),
        assistant_finished(4, "agent-2", "req-2"),
    ];

    let result = find_cut_point(&events, "agent-2", 1000).unwrap();
    assert_eq!(result.first_kept_event_seq, 3);
}

#[test]
fn find_cut_point_no_valid_cut_points_keeps_from_first() {
    // Only stream deltas — no UserMessageSubmitted or AssistantMessageFinished
    let events = vec![
        stream_delta(1, "agent-1", "req-1", "hello"),
        stream_delta(2, "agent-1", "req-1", "world"),
    ];

    let result = find_cut_point(&events, "agent-1", 1).unwrap();
    assert_eq!(result.first_kept_event_seq, 1);
    assert!(!result.is_split_turn);
}

// =========================================================================
// Ported from Pi's compaction.test.ts
// =========================================================================

#[test]
fn pi_calculate_context_tokens_matches() {
    // Pi test: createMockUsage(1000, 500, 200, 100) -> 1800
    assert_eq!(calculate_context_tokens(1000, 500, 200, 100), 1800);
}

#[test]
fn pi_calculate_context_tokens_zero() {
    // Pi test: createMockUsage(0, 0, 0, 0) -> 0
    assert_eq!(calculate_context_tokens(0, 0, 0, 0), 0);
}

#[test]
fn pi_should_compact_boundary() {
    // Pi test: shouldCompact(95000, 100000, settings) -> true
    // Pi test: shouldCompact(89000, 100000, settings) -> false
    let s = settings(true, 10000, 20000);
    assert!(should_compact(95000, 100000, &s));
    assert!(!should_compact(89000, 100000, &s));
}

#[test]
fn pi_should_compact_disabled() {
    // Pi test: disabled returns false even at 95000/100000
    let s = settings(false, 10000, 20000);
    assert!(!should_compact(95000, 100000, &s));
}

#[test]
fn pi_find_cut_point_finds_valid_cut_point() {
    // Port of Pi's "should find cut point based on actual token differences"
    let mut events = Vec::new();
    for i in 0u64..10 {
        events.push(user_msg(
            i * 2 + 1,
            "agent-1",
            &format!("req-{i}"),
            &format!("User {i}"),
        ));
        events.push(stream_delta(
            i * 2 + 2,
            "agent-1",
            &format!("req-{i}"),
            &format!("Assistant {i}"),
        ));
        events.push(assistant_finished(
            i * 2 + 3,
            "agent-1",
            &format!("req-{i}"),
        ));
    }

    let result = find_cut_point(&events, "agent-1", 25).unwrap();

    let cut_event = events
        .iter()
        .find(|e| e.seq == result.first_kept_event_seq)
        .unwrap();
    assert!(
        matches!(
            cut_event.payload,
            EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_)
        ),
        "cut point must be user or assistant event"
    );
}

#[test]
fn pi_find_cut_point_keeps_recent_turn_if_fits() {
    // Port of Pi's "should keep everything if all messages fit within budget",
    // adapted for harness: when the whole context fits, still keep the most
    // recent turn so there is prior context to summarize.
    let events = vec![
        user_msg(1, "agent-1", "req-1", "1"),
        stream_delta(2, "agent-1", "req-1", "a"),
        assistant_finished(3, "agent-1", "req-1"),
        user_msg(4, "agent-1", "req-2", "2"),
        stream_delta(5, "agent-1", "req-2", "b"),
        assistant_finished(6, "agent-1", "req-2"),
    ];

    let result = find_cut_point(&events, "agent-1", 50000).unwrap();
    assert_eq!(result.first_kept_event_seq, 4);
}

// =========================================================================
// build_session_context
// =========================================================================

fn provider_started(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: RequestId::new(request_id),
            provider_id: "test-provider".to_string(),
            model_id: "test-model".to_string(),
            prompt_summary: "test prompt".to_string(),
            request_digest: "digest".to_string(),
            metadata: None,
        }),
    )
}

fn session_compaction(
    seq: u64,
    agent_id: &str,
    summary: &str,
    first_kept_event_seq: u64,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::SessionCompaction(SessionCompactionEvent {
            agent_id: agent_id.to_string(),
            summary: summary.to_string(),
            first_kept_event_seq,
            first_kept_request_id: None,
            tokens_before: 100,
            read_files: Vec::new(),
            modified_files: Vec::new(),
            trigger_reason: "auto".to_string(),
            from_hook: false,
        }),
    )
}

fn branch_summary_event(seq: u64, agent_id: &str, summary: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        agent_id,
        EventV1::BranchSummary(BranchSummaryEvent {
            agent_id: agent_id.to_string(),
            summary: summary.to_string(),
            from_event_seq: seq,
            read_files: Vec::new(),
            modified_files: Vec::new(),
            from_hook: false,
        }),
    )
}

fn full_turn(
    start_seq: u64,
    agent_id: &str,
    request_id: &str,
    user_text: &str,
    assistant_text: &str,
) -> Vec<EventEnvelopeV1> {
    vec![
        user_msg(start_seq, agent_id, request_id, user_text),
        provider_started(start_seq + 1, agent_id, request_id),
        stream_delta(start_seq + 2, agent_id, request_id, assistant_text),
        assistant_finished(start_seq + 3, agent_id, request_id),
    ]
}

#[test]
fn build_session_context_no_compaction_returns_all_messages() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "hello", "hi there"),
        full_turn(5, "agent-1", "req-2", "how are you", "great"),
    ]
    .concat();

    let messages = build_session_context(&events, "agent-1");

    assert_eq!(messages.len(), 4);
    assert!(matches!(
        &messages[0],
        ConversationMessage::User(u) if u.text == "hello"
    ));
    assert!(matches!(
        &messages[1],
        ConversationMessage::Assistant(a) if a.text == "hi there"
    ));
    assert!(matches!(
        &messages[2],
        ConversationMessage::User(u) if u.text == "how are you"
    ));
    assert!(matches!(
        &messages[3],
        ConversationMessage::Assistant(a) if a.text == "great"
    ));
}

#[test]
fn build_session_context_with_compaction_includes_summary_and_kept_messages() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "first", "response1"),
        full_turn(5, "agent-1", "req-2", "second", "response2"),
        vec![session_compaction(
            9,
            "agent-1",
            "Summary of first two turns",
            5,
        )],
        full_turn(10, "agent-1", "req-3", "third", "response3"),
    ]
    .concat();

    let messages = build_session_context(&events, "agent-1");

    assert_eq!(messages.len(), 5);

    assert!(matches!(
        &messages[0],
        ConversationMessage::User(u) if u.text.contains("Summary of first two turns")
    ));

    assert!(matches!(
        &messages[1],
        ConversationMessage::User(u) if u.text == "second"
    ));
    assert!(matches!(
        &messages[2],
        ConversationMessage::Assistant(a) if a.text == "response2"
    ));
    assert!(matches!(
        &messages[3],
        ConversationMessage::User(u) if u.text == "third"
    ));
    assert!(matches!(
        &messages[4],
        ConversationMessage::Assistant(a) if a.text == "response3"
    ));
}

#[test]
fn build_session_context_multiple_compactions_uses_latest() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "a", "b"),
        vec![session_compaction(5, "agent-1", "First summary", 1)],
        full_turn(6, "agent-1", "req-2", "c", "d"),
        vec![session_compaction(10, "agent-1", "Second summary", 6)],
        full_turn(11, "agent-1", "req-3", "e", "f"),
    ]
    .concat();

    let messages = build_session_context(&events, "agent-1");

    assert_eq!(messages.len(), 5);

    assert!(matches!(
        &messages[0],
        ConversationMessage::User(u) if u.text.contains("Second summary")
    ));
    assert!(matches!(
        &messages[1],
        ConversationMessage::User(u) if u.text == "c"
    ));
    assert!(matches!(
        &messages[2],
        ConversationMessage::Assistant(a) if a.text == "d"
    ));
    assert!(matches!(
        &messages[3],
        ConversationMessage::User(u) if u.text == "e"
    ));
    assert!(matches!(
        &messages[4],
        ConversationMessage::Assistant(a) if a.text == "f"
    ));
}

#[test]
fn build_session_context_compaction_keeping_from_first_message() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "first", "response"),
        vec![session_compaction(5, "agent-1", "Empty summary", 1)],
        full_turn(6, "agent-1", "req-2", "second", "resp2"),
    ]
    .concat();

    let messages = build_session_context(&events, "agent-1");

    assert_eq!(messages.len(), 5);
    assert!(matches!(
        &messages[0],
        ConversationMessage::User(u) if u.text.contains("Empty summary")
    ));
    assert!(matches!(
        &messages[1],
        ConversationMessage::User(u) if u.text == "first"
    ));
}

#[test]
fn build_session_context_filters_by_agent_id() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "hello", "hi"),
        full_turn(5, "agent-2", "req-2", "other", "agent"),
    ]
    .concat();

    let messages = build_session_context(&events, "agent-1");

    assert_eq!(messages.len(), 2);
    assert!(matches!(
        &messages[0],
        ConversationMessage::User(u) if u.text == "hello"
    ));
    assert!(matches!(
        &messages[1],
        ConversationMessage::Assistant(a) if a.text == "hi"
    ));
}

#[test]
fn build_session_context_empty_events_returns_empty() {
    let messages = build_session_context(&[], "agent-1");
    assert!(messages.is_empty());
}

// =========================================================================
// build_session_context_with_branch_summaries
// =========================================================================

#[test]
fn build_session_context_with_branch_summaries_injects_summary() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "start", "response"),
        vec![branch_summary_event(
            5,
            "agent-1",
            "Summary of abandoned work",
        )],
        full_turn(6, "agent-1", "req-2", "new direction", "resp2"),
    ]
    .concat();

    let messages = build_session_context_with_branch_summaries(&events, "agent-1");

    assert_eq!(messages.len(), 5);

    assert!(matches!(
        &messages[0],
        ConversationMessage::User(u) if u.text == "start"
    ));
    assert!(matches!(
        &messages[1],
        ConversationMessage::Assistant(a) if a.text == "response"
    ));
    assert!(matches!(
        &messages[2],
        ConversationMessage::User(u) if u.text.contains("Summary of abandoned work")
            && u.text.contains("<summary>")
    ));
    assert!(matches!(
        &messages[3],
        ConversationMessage::User(u) if u.text == "new direction"
    ));
    assert!(matches!(
        &messages[4],
        ConversationMessage::Assistant(a) if a.text == "resp2"
    ));
}

#[test]
fn build_session_context_with_branch_summaries_and_compaction() {
    let events: Vec<EventEnvelopeV1> = [
        full_turn(1, "agent-1", "req-1", "first", "r1"),
        full_turn(5, "agent-1", "req-2", "second", "r2"),
        vec![session_compaction(9, "agent-1", "Compacted history", 5)],
        vec![branch_summary_event(10, "agent-1", "Branch context")],
        full_turn(11, "agent-1", "req-3", "third", "r3"),
    ]
    .concat();

    let messages = build_session_context_with_branch_summaries(&events, "agent-1");

    assert!(messages.len() >= 5);
    assert!(messages.iter().any(|m| matches!(m,
        ConversationMessage::User(u) if u.text.contains("Compacted history")
    )));
    assert!(messages.iter().any(|m| matches!(m,
        ConversationMessage::User(u) if u.text.contains("Branch context")
    )));
}

// =========================================================================
// estimate_session_context_tokens
// =========================================================================

#[test]
fn estimate_session_context_tokens_returns_nonzero_for_nonempty_context() {
    let events: Vec<EventEnvelopeV1> =
        [full_turn(1, "agent-1", "req-1", "abcdefgh", "ijklmnop")].concat();

    let estimate = estimate_session_context_tokens(&events, "agent-1");

    assert!(estimate.total_tokens > 0);
    assert!(estimate.estimated);
}

#[test]
fn estimate_session_context_tokens_zero_for_empty_events() {
    let estimate = estimate_session_context_tokens(&[], "agent-1");

    assert_eq!(estimate.total_tokens, 0);
    assert!(estimate.estimated);
}
