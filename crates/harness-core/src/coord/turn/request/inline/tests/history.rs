//! Prefix replay: first retractor, rewrite stamps and dropped provider continuations.
use super::*;
use crate::coord::context::archive::inline_cache::InlineCache;
use crate::coord::turn::request::inline::analysis::{analyze, apply, stamp_rewrites};
use harness_providers::{ContinuationExport, ProviderContinuation};
use std::sync::Arc;

fn signed(message: &mut CompletionMessage) -> Result<(), Box<dyn std::error::Error>> {
    message.continuation = Some(Box::new(ProviderContinuation::import(
        ContinuationExport {
            protocol: "responses".into(),
            provider_id: Some("mock".into()),
            model_id: "m".into(),
            data: serde_json::json!([{"type": "compaction", "encrypted_content": "opaque"}]),
        },
    )?));
    Ok(())
}

/// Message indices: user, then per result: assistant call and tool result; `shots` adds
/// a user message carrying that many images after the given assistant.
struct Scenario {
    messages: Vec<CompletionMessage>,
    attachments: Attachments,
}

fn scenario(shots: &[usize]) -> Result<Scenario, Box<dyn std::error::Error>> {
    // 0 user; 1 call a; 2 result a; 3 assistant; 4 call b; 5 result b; 6 assistant
    let mut messages = vec![
        message(MessageRole::User, "go"),
        call("a", "grep"),
        tool("a", &text("a", 8_000)),
        message(MessageRole::Assistant, "thought about a"),
        call("b", "grep"),
        tool("b", &text("b", 8_000)),
        message(MessageRole::Assistant, "thought about b"),
    ];
    for message in &mut messages {
        if message.role == MessageRole::Assistant {
            signed(message)?;
        }
    }
    let mut attachments = Attachments::new();
    for (n, images) in shots.iter().enumerate() {
        let index = messages.len();
        messages.push(message(MessageRole::User, "screenshot"));
        let ids: Vec<String> = (0..*images).map(|i| format!("shot{n}-{i}")).collect();
        attach(
            &mut attachments,
            index,
            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        messages.push(message(MessageRole::Assistant, "saw it"));
    }
    Ok(Scenario {
        messages,
        attachments,
    })
}

fn run(shots: &[usize], limit: u32) -> Result<(Scenario, InlineCache), Box<dyn std::error::Error>> {
    run_with(shots, limit, |_| {})
}

fn run_with(
    shots: &[usize],
    limit: u32,
    tweak: impl FnOnce(&mut [Fact]),
) -> Result<(Scenario, InlineCache), Box<dyn std::error::Error>> {
    let counter = counter();
    let env = env(&counter, limit, ArchiveSystemPrompt::None, true);
    let mut cache = with_sink(Arc::new(FakeSink::new(&[("a", 1), ("b", 1)])));
    let mut scenario = scenario(shots)?;
    let mut flags = facts(scenario.messages.len());
    tweak(&mut flags);
    let analysis = analyze(
        &env,
        &scenario.messages,
        &scenario.attachments,
        &flags,
        &mut cache,
        false,
    );
    stamp_rewrites(&analysis, &scenario.messages, &flags, &mut cache);
    let mut content = empty_content();
    apply(
        &analysis,
        &mut scenario.messages,
        &mut scenario.attachments,
        &mut content,
    );
    Ok((scenario, cache))
}

#[test]
fn a_result_retracted_after_an_assistant_replied_is_stamped_and_drops_old_thinking() -> Test {
    // limit 3: a and b fit (1 frame each); a screenshot with 2 images pushes b out.
    let (scenario, cache) = run(&[2], 3)?;
    // b (call id "b", seq 6) was retracted by the screenshot at index 7 (time 1070).
    assert_eq!(cache.stamp_for(6, Some("b")), Some(1_070));
    assert_eq!(cache.stamp_for(3, Some("a")), None);
    // The assistant after b replied against the framed b: its continuation goes.
    assert!(scenario.messages[6].continuation.is_none());
    // Earlier assistants never saw b framed.
    assert!(scenario.messages[3].continuation.is_some());
    assert_eq!(scenario.messages[5].content, text("b", 8_000));
    Ok(())
}

#[test]
fn an_existing_carrier_rewrite_time_is_part_of_the_stamp_maximum() -> Test {
    for (existing, expected) in [(Some(9_000), 9_000), (Some(10), 1_070), (None, 1_070)] {
        let (_, cache) = run_with(&[2], 3, |facts| facts[5].history_rewrite_at_ms = existing)?;
        assert_eq!(cache.stamp_for(6, Some("b")), Some(expected));
    }
    Ok(())
}

#[test]
fn the_first_forcing_message_not_an_earlier_crossing_stamps_a_later_retraction() -> Test {
    // First screenshot (2 images) retracts b at 7; the second (1 image) then retracts a.
    let (scenario, cache) = run(&[2, 1], 3)?;
    assert_eq!(cache.stamp_for(6, Some("b")), Some(1_070));
    assert_eq!(cache.stamp_for(3, Some("a")), Some(1_090));
    // Both thoughts replied against a framed result that is now text.
    assert!(scenario.messages[3].continuation.is_none());
    assert!(scenario.messages[6].continuation.is_none());
    Ok(())
}

#[test]
fn no_stamp_without_an_assistant_between_the_result_and_its_retractor() -> Test {
    let counter = counter();
    let env = env(&counter, 2, ArchiveSystemPrompt::None, true);
    let mut cache = with_sink(Arc::new(FakeSink::new(&[("a", 2)])));
    let mut messages = vec![
        message(MessageRole::User, "go"),
        call("a", "grep"),
        tool("a", &text("a", 8_000)),
        message(MessageRole::User, "screenshot"),
    ];
    let mut attachments = Attachments::new();
    attach(&mut attachments, 3, &["shot"]);
    let flags = facts(4);
    let analysis = analyze(&env, &messages, &attachments, &flags, &mut cache, false);
    stamp_rewrites(&analysis, &messages, &flags, &mut cache);
    assert_eq!(analysis.retractions.len(), 1);
    assert!(cache.stamps.is_empty());
    let mut content = empty_content();
    apply(&analysis, &mut messages, &mut attachments, &mut content);
    assert!(!content.contains_key(&2));
    Ok(())
}

#[test]
fn a_stamp_never_moves_backwards() -> Test {
    let mut cache = InlineCache::default();
    cache.stamp(1, Some("x"), 500);
    cache.stamp(1, Some("x"), 300);
    assert_eq!(cache.stamp_for(1, Some("x")), Some(500));
    assert_eq!(cache.stamp_for(1, None), None);
    Ok(())
}

type Test = Result<(), Box<dyn std::error::Error>>;
