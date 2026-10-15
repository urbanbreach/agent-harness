//! Mixed tool result application and the `read` opt-in.
use super::*;
use crate::coord::context::archive::inline_cache::InlineCache;
use crate::coord::turn::request::inline::analysis::{analyze, apply};
use crate::coord::turn::request::inline::reads_images;
use harness_providers::MessageContentPart as Part;
use std::sync::Arc;

#[test]
fn a_mixed_result_keeps_every_original_image_after_the_frames() {
    let counter = counter();
    let env = env(&counter, 90, ArchiveSystemPrompt::None, true);
    let sink = Arc::new(FakeSink::new(&[("big", 2)]));
    let mut cache = with_sink(Arc::clone(&sink));
    let mut messages = vec![
        message(MessageRole::User, "go"),
        call("c1", "shot"),
        tool("c1", &text("big", 8_000)),
    ];
    let mut attachments = Attachments::new();
    attach(&mut attachments, 2, &["first", "second"]);
    let sent = (messages.clone(), attachments.clone());
    let analysis = analyze(&env, &messages, &attachments, &facts(3), &mut cache, false);
    // Planning never changes the inputs.
    assert_eq!((messages.clone(), attachments.clone()), sent);
    let mut content = empty_content();
    apply(&analysis, &mut messages, &mut attachments, &mut content);

    let rendered = sink
        .texts
        .lock()
        .map(|texts| texts.clone())
        .unwrap_or_default();
    assert!(rendered[0].ends_with(
        "\n[Source image 1 was attached here in the original tool result.]\n[Source image 2 was attached here in the original tool result.]"
    ));
    let ids: Vec<_> = attachments[&2].iter().map(|a| a.id.as_str()).collect();
    assert!(ids[0].starts_with("inline-tool-") && ids[1].starts_with("inline-tool-"));
    assert_eq!(ids[2..], ["first", "second"]);
    let label = |n: usize| {
        Part::Text(format!(
            "[Original source image {n}; corresponds to its marker in the compacted text.]"
        ))
    };
    let parts = &content[&2];
    assert!(matches!(parts[0], Part::Text(_)));
    assert_eq!(
        parts[1..],
        [
            Part::Attachment(0),
            Part::Attachment(1),
            label(1),
            Part::Attachment(2),
            label(2),
            Part::Attachment(3),
        ]
    );
    assert!(messages[2].content.contains("[Original source image 2;"));
}

#[test]
fn read_output_images_only_for_an_opted_in_reader() {
    let counter = counter();
    let build = |opt_in: bool, error: bool| {
        let mut env = env(&counter, 90, ArchiveSystemPrompt::None, true);
        env.read_opt_in = opt_in;
        let mut cache = with_sink(Arc::new(FakeSink::new(&[])));
        let messages = vec![
            message(MessageRole::User, "go"),
            call("r1", "read"),
            tool("r1", &text("file", 9_000)),
            call("g1", "grep"),
            tool("g1", &text("match", 9_000)),
        ];
        let mut flags = facts(5);
        flags[2].is_error = error;
        flags[4].is_error = error;
        let analysis = analyze(
            &env,
            &messages,
            &Attachments::new(),
            &flags,
            &mut cache,
            false,
        );
        analysis
            .tools
            .built
            .iter()
            .map(|tool| tool.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(build(false, false), ["g1"]);
    assert_eq!(build(true, false), ["r1", "g1"]);
    assert!(build(true, true).is_empty());
}

#[test]
fn only_opus_five_five_up_to_ten_reads_images() {
    let rows = [
        ("anthropic", "claude-opus-5-5", true),
        ("anthropic", "claude-opus-6", true),
        ("anthropic", "claude-opus-5-4", false),
        ("anthropic", "claude-opus-4-7", false),
        ("anthropic", "claude-sonnet-5-5", false),
        ("anthropic", "claude-opus-10", false),
        ("openai", "gpt-5.5", false),
    ];
    for (provider, model, expected) in rows {
        assert_eq!(reads_images(provider, model), expected, "{model}");
    }
}
