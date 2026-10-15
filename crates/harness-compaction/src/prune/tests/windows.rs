use super::item;
use crate::prune::lookback_floor;
use harness_providers::{CompletionMessage, MessageRole};

fn conversation(length: usize) -> Vec<CompletionMessage> {
    (0..length)
        .map(|index| {
            CompletionMessage::text(
                if index % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                format!("turn {index}"),
            )
        })
        .collect()
}

#[test]
fn lookback_reserves_six_positions_and_keeps_the_issuing_assistant_reachable() {
    // Lookback 20: 13 stored positions fit (6 + 13 < 20), 14 reach the limit.
    for (length, lookback, expected) in [
        (13, Some(20), 0),
        (14, Some(20), 1),
        (30, None, 0),
        (3, Some(6), 3),
    ] {
        let messages = conversation(length);
        let items: Vec<_> = messages.iter().map(item).collect();
        assert_eq!(
            lookback_floor(&items, 0, lookback),
            expected,
            "length {length} lookback {lookback:?}"
        );
    }
}

#[test]
fn parallel_tool_batches_and_error_images_count_as_runs() {
    let mut messages = vec![CompletionMessage::text(MessageRole::User, "go")];
    let mut call = CompletionMessage::text(MessageRole::Assistant, "plan");
    call.assistant_tool_calls = Some(
        (0..8)
            .map(|i| harness_providers::AssistantToolCall {
                tool_call_id: i.to_string(),
                function_name: "read".into(),
                arguments_json: "{}".into(),
            })
            .collect(),
    );
    messages.push(call);
    for i in 0..8 {
        let mut result = CompletionMessage::text(MessageRole::Tool, "ok");
        result.tool_call_id = Some(i.to_string());
        messages.push(result);
    }
    let items: Vec<_> = messages.iter().map(item).collect();
    // user (1) + assistant text and calls (2) + one result run (1) + 6 reserved = 10.
    assert_eq!(lookback_floor(&items, 0, Some(11)), 0);
    assert_eq!(lookback_floor(&items, 0, Some(10)), 1);
}

#[test]
fn an_assistant_that_reaches_the_lookback_does_not_become_the_floor() {
    // 14 entries starting with an assistant: the oldest one is the entry that reaches
    // 6 reserved + 14 = 20, so the issuing turn does not fit and the floor is the next
    // assistant that did (index 2), never the one that overflowed.
    let mut messages = conversation(14);
    messages.rotate_left(1);
    let items: Vec<_> = messages.iter().map(item).collect();
    assert!(items[0].is_assistant() && items[1].is_user());
    assert_eq!(lookback_floor(&items, 0, Some(20)), 2);
}
