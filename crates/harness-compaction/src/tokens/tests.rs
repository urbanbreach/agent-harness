use super::*;
use crate::prepare::tests::item;
use crate::{ImageMeta, ImageOrigin, ItemPart};
use harness_providers::{
    AssistantToolCall, CompletionMessage, ContinuationExport, MessageRole, ProviderContinuation,
};
use harness_tokenizer::{Counter, Reader, ReaderApi};
use serde_json::{json, Value};

mod reasoning;

fn user(text: impl Into<String>) -> CompletionMessage {
    CompletionMessage::text(MessageRole::User, text)
}

pub(super) fn assistant(text: impl Into<String>) -> CompletionMessage {
    CompletionMessage::text(MessageRole::Assistant, text)
}

pub(super) fn counter(api: ReaderApi, provider: &str, model: &str) -> Counter {
    Counter::byte_estimating(Some(Reader {
        api,
        provider: provider.into(),
        model_id: model.into(),
        official_endpoint: true,
        prefix_binding: false,
        filter_reasoning_history: false,
    }))
}

pub(super) fn continuation(
    protocol: &str,
    provider: &str,
    model: &str,
    data: Value,
) -> ProviderContinuation {
    let export = ContinuationExport {
        protocol: protocol.into(),
        provider_id: Some(provider.into()),
        model_id: model.into(),
        data,
    };
    ProviderContinuation::import(export).expect("continuation within caps")
}

fn usage(prompt: u32, completion: u32, total: u32) -> CompletionUsage {
    CompletionUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: total,
        ..Default::default()
    }
}

#[test]
fn usage_anchor_and_local_fallback_parity() {
    let messages = [
        user("ancient"),
        assistant("answer"),
        user("x".repeat(4_000)),
        assistant("error"),
    ];
    let good = usage(1_195, 5, 1_200);
    let zero = CompletionUsage::default();
    let mut items = messages.iter().map(item).collect::<Vec<_>>();
    items[1].usage = Some(&good);
    items[3].usage = Some(&good);
    for (stop, anchor) in [(Stop::Error, 1), (Stop::Aborted, 1), (Stop::Truncated, 3)] {
        items[3].stop = stop;
        assert_eq!(
            usage_anchor(&items, 0, false),
            Some(Anchor {
                index: anchor,
                tokens: 1_200
            })
        );
    }
    items[3].usage = Some(&zero);
    items[3].stop = Stop::Complete;
    assert_eq!(usage_anchor(&items, 2, false), None);
    assert_eq!(transcript_estimate(&items[..3], 0, 0, false, false), 2_200);
    assert_eq!(transcript_estimate(&items[..3], 3, 0, false, false), 1_004);
    let users = [user("x".repeat(400)), user("x".repeat(400))];
    let users = users.iter().map(item).collect::<Vec<_>>();
    for (from, expected) in [(0, 200), (1, 100)] {
        assert_eq!(transcript_estimate(&users, 0, from, false, false), expected);
    }
}

#[test]
fn encrypted_exclusion_switches_the_local_tail_domain() {
    let messages = [assistant("a"), assistant("b")];
    let mut items = messages.iter().map(item).collect::<Vec<_>>();
    items[1].tokens = 3_000;
    items[1].tokens_floor = 9;
    assert_eq!(
        transcript_estimate(&items, 0, 0, false, false),
        3_000 + items[0].tokens
    );
    assert_eq!(
        transcript_estimate(&items, 0, 0, true, false),
        9 + items[0].tokens_floor
    );
}

#[test]
fn pruned_anchors_are_skipped_and_rewrites_retire_request_anchors() {
    let messages = [user("q"), assistant("a1"), user("r"), assistant("a2")];
    let usage = usage(100, 5, 105);
    let mut items = messages.iter().map(item).collect::<Vec<_>>();
    items[1].usage = Some(&usage);
    items[1].at_ms = Some(10);
    items[3].usage = Some(&usage);
    items[3].at_ms = Some(30);
    items[2].pruned_at_ms = Some(20);
    let anchor = |index| Some(Anchor { index, tokens: 105 });
    assert_eq!(usage_anchor(&items, 0, true), anchor(3));
    assert_eq!(usage_anchor(&items[..3], 0, true), None);
    assert_eq!(usage_anchor(&items[..3], 0, false), anchor(1));
    // The prune at 20 retires the anchor stamped 10 but not the one stamped 30.
    assert_eq!(request_anchor(&items, &[]), anchor(3));
    // A later rewrite marker does not clear an anchor that was already saved.
    let marker = RewriteMarker {
        index: 2,
        at_ms: 40,
    };
    assert_eq!(request_anchor(&items, &[marker]), anchor(1));
    let early = RewriteMarker {
        index: 0,
        at_ms: 35,
    };
    assert_eq!(request_anchor(&items, &[early]), None);
}

#[test]
fn provider_usage_arithmetic_follows_the_contract() {
    let split = CompletionUsage {
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 100,
        ..Default::default()
    };
    let cases = [
        (usage(110, 5, 115), 115, 110, true),
        (usage(110, 5, 0), 115, 110, true),
        (split.clone(), 115, 110, true),
        (
            CompletionUsage {
                context_tokens: Some(0),
                total_tokens: 500,
                ..Default::default()
            },
            0,
            0,
            false,
        ),
        (usage(186_765, 29, 186_794), 186_794, 186_765, true),
        (CompletionUsage::default(), 0, 0, false),
    ];
    for (usage, context, prompt, anchors) in cases {
        assert_eq!(context_tokens(&usage), context);
        assert_eq!(prompt_tokens(&usage), prompt);
        assert_eq!(has_context_usage(&usage), anchors);
    }
    assert_eq!(compaction_context_tokens(100, 200), 200);
}

#[test]
fn anchored_savings_only_count_entries_strictly_before_the_anchor() {
    let anchor = Anchor {
        index: 5,
        tokens: 1_000,
    };
    let savings = [(1, 10), (3, 20), (5, 30), (7, 40)];
    assert_eq!(anchored_savings(anchor, &savings, None), 30);
    assert_eq!(anchored_savings(anchor, &savings, Some(2)), 20);
    let mut usage = CompletionUsage {
        input_tokens: 5,
        cache_read_tokens: 95,
        prompt_tokens: 100,
        total_tokens: 110,
        context_tokens: Some(110),
        ..Default::default()
    };
    correct_usage(&mut usage, 30);
    assert_eq!(
        (
            usage.total_tokens,
            usage.prompt_tokens,
            usage.context_tokens
        ),
        (80, 70, Some(80))
    );
    assert_eq!((usage.input_tokens, usage.cache_read_tokens), (0, 70));
}

#[test]
fn messages_price_each_block_without_framing() {
    let c = counter(ReaderApi::OpenAiResponses, "custom", "plain-model");
    let calls = vec![AssistantToolCall {
        tool_call_id: "id".into(),
        function_name: "read".into(),
        arguments_json: "{ \"path\" : \"a.ts\" }".into(),
    }];
    let mut call = assistant("");
    call.assistant_tool_calls = Some(calls);
    let blank = AssistantToolCall {
        tool_call_id: "b".into(),
        function_name: "go".into(),
        arguments_json: "  ".into(),
    };
    let mut nullish = assistant("");
    nullish.assistant_tool_calls = Some(vec![blank]);
    let tool = CompletionMessage::text(MessageRole::Tool, "x".repeat(5));
    let prompt = user("abcde");
    let system = CompletionMessage::text(MessageRole::System, "x".repeat(40));
    // Tool calls on a non-assistant role are not priced.
    let mut stray = user("");
    stray.assistant_tool_calls = call.assistant_tool_calls.clone();
    let invalid = AssistantToolCall {
        tool_call_id: "i".into(),
        function_name: "go".into(),
        arguments_json: "{ not json".into(),
    };
    let mut broken = assistant("");
    broken.assistant_tool_calls = Some(vec![invalid]);
    let cases = [
        (item(&prompt), 2),
        (item(&tool), 2),
        (item(&system), 0),
        (item(&stray), 0),
        // Text that does not parse is priced as it is sent.
        (item(&broken), 1 + 3),
        // Name and compact arguments are separate fragments: 1 + 4 tokens.
        (item(&call), 5),
        // Empty arguments price as the literal `null`.
        (item(&nullish), 2),
    ];
    for (item, expected) in cases {
        assert_eq!(
            estimate_item(&c, &item),
            ItemTokens {
                normal: expected,
                floor: expected
            }
        );
    }
    let reasoning = vec!["abcd".to_owned(), "abcdefghi".to_owned()];
    let message = assistant("abcd");
    let mut flat = item(&message);
    flat.reasoning = &reasoning;
    assert_eq!(estimate_item(&c, &flat).normal, 1 + 1 + 3);
    let parts = [
        ItemPart::text("abcd"),
        ItemPart::text(""),
        ItemPart::reasoning("abcde"),
        ItemPart::Call(0),
    ];
    let mut ordered = item(&call);
    ordered.parts = &parts;
    assert_eq!(estimate_item(&c, &ordered).normal, 1 + 2 + 5);
}

#[test]
fn images_price_by_origin() {
    let c = counter(ReaderApi::Anthropic, "anthropic", "claude-opus-4-7");
    let message = user("");
    let images = [
        ImageMeta {
            mime: "image/png".into(),
            width: Some(1000),
            height: Some(1000),
            detail: None,
            origin: ImageOrigin::User,
        },
        ImageMeta {
            mime: "image/png".into(),
            width: Some(800),
            height: Some(600),
            detail: None,
            origin: ImageOrigin::ArchiveFrame,
        },
        ImageMeta {
            mime: "image/png".into(),
            width: None,
            height: None,
            detail: None,
            origin: ImageOrigin::ArchiveFrame,
        },
    ];
    let mut entry = item(&message);
    entry.images = &images;
    assert_eq!(estimate_item(&c, &entry).normal, 1296 + 29 * 22 + 5024);
}

#[test]
fn summaries_price_raw_text_blocks_and_frames() {
    let c = counter(ReaderApi::Anthropic, "anthropic", "claude-opus-4-7");
    assert_eq!(
        summary_price(&c, "abcd", ["abcdefgh"], [(800, 600)]),
        1 + 2 + 29 * 22
    );
    assert_eq!(summary_price(&c, "", [], [(0, 0)]), 5024);
}
