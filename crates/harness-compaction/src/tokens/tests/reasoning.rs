use super::super::*;
use super::{assistant, continuation, counter, item};
use crate::Stop;
use harness_providers::{AssistantToolCall, CompletionMessage, CompletionUsage};
use harness_tokenizer::{Counter, Reader, ReaderApi};
use serde_json::{json, Value};

fn signed_turn(stop: Stop, calls: bool) -> (CompletionMessage, CompletionUsage, Stop) {
    let mut message = assistant("x".repeat(36));
    message.continuation = Some(Box::new(continuation(
        "anthropic",
        "anthropic",
        "claude-opus-4-7",
        if stop == Stop::Aborted {
            json!([{"type": "thinking", "thinking": "hm", "signature": "s".repeat(4_000)}])
        } else {
            json!([
                {"type": "thinking", "thinking": "hm", "signature": "s".repeat(4_000)},
                {"type": "text", "text": "x"}
            ])
        },
    )));
    if calls {
        message.assistant_tool_calls = Some(vec![AssistantToolCall {
            tool_call_id: "c".into(),
            function_name: "t".into(),
            arguments_json: "{}".into(),
        }]);
    }
    let usage = CompletionUsage {
        output_tokens: 3_000,
        completion_tokens: 3_000,
        ..Default::default()
    };
    (message, usage, stop)
}

#[test]
fn signed_anthropic_reasoning_prices_by_output_size_on_a_matching_reader() {
    let reader = |api, provider: &str, model: &str, filtered| {
        let mut c = Reader {
            api,
            provider: provider.into(),
            model_id: model.into(),
            official_endpoint: false,
            prefix_binding: false,
            filter_reasoning_history: filtered,
        };
        c.official_endpoint = false;
        Counter::byte_estimating(Some(c))
    };
    let (message, usage, _) = signed_turn(Stop::Complete, false);
    let mut entry = item(&message);
    entry.usage = Some(&usage);
    let same = reader(ReaderApi::Anthropic, "anthropic", "claude-opus-4-7", false);
    assert_eq!(
        estimate_item(&same, &entry),
        ItemTokens {
            normal: 3_000,
            floor: 9
        }
    );
    // After a reducer removed 1998 output tokens, the replayed output shrinks with it.
    entry.elided_output_tokens = 1_998;
    assert_eq!(estimate_item(&same, &entry).normal, 1_002);
    entry.elided_output_tokens = 0;
    for other in [
        reader(ReaderApi::Anthropic, "bedrock", "claude-opus-4-7", false),
        reader(ReaderApi::Anthropic, "anthropic", "claude-opus-4-6", false),
        reader(ReaderApi::Anthropic, "anthropic", "claude-opus-4-7", true),
    ] {
        assert_eq!(
            estimate_item(&other, &entry),
            ItemTokens {
                normal: 9,
                floor: 9
            }
        );
    }
    // Official endpoints accept signed reasoning across models.
    let mut official = Reader {
        api: ReaderApi::Anthropic,
        provider: "anthropic".into(),
        model_id: "claude-opus-4-6".into(),
        official_endpoint: true,
        prefix_binding: false,
        filter_reasoning_history: false,
    };
    assert_eq!(
        estimate_item(&Counter::byte_estimating(Some(official.clone())), &entry).normal,
        3_000
    );
    official.official_endpoint = false;
    assert_eq!(
        estimate_item(&Counter::byte_estimating(Some(official)), &entry).normal,
        9
    );
    // A final signed block of an aborted turn, or tool calls on a length stop, are untrusted.
    for (stop, calls) in [(Stop::Aborted, false), (Stop::Truncated, true)] {
        let (message, usage, stop) = signed_turn(stop, calls);
        let mut entry = item(&message);
        entry.usage = Some(&usage);
        entry.stop = stop;
        assert_eq!(
            estimate_item(&same, &entry).normal,
            9 + u32::from(calls) * 2
        );
    }
}

#[test]
fn other_lineages_price_signatures_as_text_in_the_normal_domain_only() {
    let c = counter(ReaderApi::OpenAiChat, "google", "gemini-3-pro");
    let mut message = assistant("x".repeat(36));
    message.continuation = Some(Box::new(continuation(
        "chat",
        "google",
        "gemini-3-pro",
        json!({"tool_calls": {"c": {"google": {"thought_signature": "s".repeat(4_000)}}}}),
    )));
    let entry = item(&message);
    assert_eq!(
        estimate_item(&c, &entry),
        ItemTokens {
            normal: 1_009,
            floor: 9
        }
    );
}

fn responses_turn(items: Value) -> CompletionMessage {
    let mut message = assistant("");
    message.assistant_tool_calls = Some(vec![AssistantToolCall {
        tool_call_id: "c1".into(),
        function_name: "read".into(),
        arguments_json: "{\"path\":\"a.ts\"}".into(),
    }]);
    message.continuation = Some(Box::new(continuation(
        "responses",
        "openai",
        "gpt-5.5",
        json!({"type": "openaiResponsesHistory", "items": items}),
    )));
    message
}

fn read_call() -> Value {
    json!({"type": "function_call", "name": "read", "call_id": "c1", "arguments": "{\"path\":\"a.ts\"}"})
}

#[test]
fn responses_reasoning_prices_by_reported_tokens_when_replayable() {
    let c = counter(ReaderApi::OpenAiResponses, "openai", "gpt-5.5");
    let cold = counter(ReaderApi::OpenAiResponses, "openai", "gpt-5.5").with_history_replay(false);
    let encrypted =
        json!({"type": "reasoning", "id": "rs_1", "encrypted_content": "abc", "summary": []});
    let plain = json!({"type": "reasoning", "content": [{"type": "reasoning_text", "text": "t"}], "summary": []});
    let reported = CompletionUsage {
        reasoning_tokens: Some(5_000),
        ..Default::default()
    };
    let both = ItemTokens {
        normal: 5_005,
        floor: 5_005,
    };
    let visible = ItemTokens {
        normal: 5,
        floor: 5,
    };
    let dropped = ItemTokens {
        normal: 5,
        floor: 5,
    };
    let cases = [
        (vec![encrypted.clone(), read_call()], &c, both),
        (vec![encrypted.clone(), read_call()], &cold, visible),
        (vec![plain.clone(), read_call()], &cold, both),
        // No reasoning item, and reasoning that no output follows, cost nothing.
        (vec![read_call()], &c, dropped),
        (vec![encrypted.clone()], &c, dropped),
        // A call with unparsable arguments is dropped, taking the pending reasoning with it.
        (
            vec![
                encrypted,
                json!({"type": "function_call", "name": "read", "arguments": "{"}),
            ],
            &c,
            dropped,
        ),
    ];
    for (items, counter, expected) in cases {
        let message = responses_turn(Value::Array(items));
        let mut entry = item(&message);
        entry.usage = Some(&reported);
        assert_eq!(estimate_item(counter, &entry), expected);
    }
    // A numeric zero is a price, not an absence: no signature fallback follows it.
    let message = responses_turn(json!([read_call()]));
    let zero = CompletionUsage {
        reasoning_tokens: Some(0),
        ..Default::default()
    };
    let mut entry = item(&message);
    entry.usage = Some(&zero);
    assert_eq!(estimate_item(&c, &entry).normal, 5);
    assert_eq!(replayed_reasoning_tokens(&c, &[entry]), 0);
}

#[test]
fn truncated_thinking_only_turns_cost_nothing() {
    let c = counter(ReaderApi::OpenAiResponses, "openai", "gpt-5.5");
    let mut message = assistant("");
    message.continuation = Some(Box::new(continuation(
        "responses",
        "openai",
        "gpt-5.5",
        json!({"type": "openaiResponsesHistory", "items": [{"type": "reasoning", "summary": []}]}),
    )));
    let reported = CompletionUsage {
        reasoning_tokens: Some(5_000),
        ..Default::default()
    };
    let mut entry = item(&message);
    entry.usage = Some(&reported);
    entry.stop = Stop::Truncated;
    assert_eq!(estimate_item(&c, &entry), ItemTokens::default());
}
