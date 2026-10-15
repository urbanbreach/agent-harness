//! System prompt targets: extraction, the six frame cap, carrier, margin and budget.
use super::*;
use crate::coord::context::archive::inline_cache::InlineCache;
use crate::coord::turn::request::inline::analysis::{analyze, apply};
use crate::coord::turn::request::inline::estimate::SystemReason;
use crate::coord::turn::request::inline::system::extract;
use harness_providers::MessageContentPart;
use std::sync::Arc;

fn system_messages(texts: &[&str]) -> Vec<CompletionMessage> {
    texts
        .iter()
        .map(|text| message(MessageRole::System, text))
        .collect()
}

#[test]
fn all_joins_every_block_and_leaves_one_stub() {
    let blocks = system_messages(&["first", "second"]);
    let target = extract(ArchiveSystemPrompt::All, &blocks).map(|t| (t.text, t.replacement.len()));
    assert_eq!(target, Some(("first\n\nsecond".to_owned(), 1)));
    assert!(extract(ArchiveSystemPrompt::All, &system_messages(&[""])).is_none());
    assert!(extract(ArchiveSystemPrompt::None, &blocks).is_none());
}

#[test]
fn context_mode_replaces_only_the_exact_regions() {
    let first = "keep a\n<repo-rules>\n  rule one \n</repo-rules>\nkeep b\n## Context\n<instructions>\nctx\n</instructions>\nend";
    // Not newline exact: never a region.
    let loose = "<repo-rules> inline </repo-rules>";
    let second = "other\n<repo-rules>\nrule two\n</repo-rules>";
    let blocks = system_messages(&[first, loose, second]);
    let target = extract(ArchiveSystemPrompt::AgentsMd, &blocks);
    assert!(target.is_some(), "regions expected");
    let Some(target) = target else {
        return;
    };
    // Pattern order per block: repo rules, then context instructions.
    assert_eq!(
        target.text,
        "<repo-rules>\n  rule one \n</repo-rules>\n\n## Context\n<instructions>\nctx\n</instructions>\n\n<repo-rules>\nrule two\n</repo-rules>"
    );
    let rest: Vec<_> = target
        .replacement
        .iter()
        .map(|m| m.content.as_str())
        .collect();
    assert_eq!(rest.len(), 3);
    assert!(rest[0].starts_with("keep a\n"));
    assert!(rest[0].contains("keep b\n") && rest[0].ends_with("\nend"));
    assert!(!rest[0].contains("rule one") && !rest[0].contains("ctx"));
    assert_eq!(rest[1], loose);
    assert_eq!(rest[2], "other\n");
    let stubs = rest
        .iter()
        .filter(|text| text.contains("context files"))
        .count();
    assert_eq!(stubs, 1);
    assert!(extract(ArchiveSystemPrompt::AgentsMd, &system_messages(&["plain"])).is_none());
}

struct Row {
    label: &'static str,
    tokens: usize,
    frames: usize,
    user: bool,
    source_images: usize,
    limit: u32,
    swapped: bool,
    reason: Option<SystemReason>,
}

#[test]
fn system_swap_needs_a_carrier_six_frames_the_margin_and_budget() {
    let row = |label, tokens, frames, user, source_images, limit, swapped, reason| Row {
        label,
        tokens,
        frames,
        user,
        source_images,
        limit,
        swapped,
        reason,
    };
    let rows = [
        row("six", 30_000, 6, true, 0, 90, true, None),
        row(
            "seven",
            30_000,
            7,
            true,
            0,
            90,
            false,
            Some(SystemReason::Budget),
        ),
        row(
            "margin",
            6_900,
            2,
            true,
            0,
            90,
            false,
            Some(SystemReason::Margin),
        ),
        row(
            "nouser",
            30_000,
            1,
            false,
            0,
            90,
            false,
            Some(SystemReason::NoCarrier),
        ),
        row(
            "room",
            30_000,
            3,
            true,
            88,
            90,
            false,
            Some(SystemReason::Budget),
        ),
        row("exact", 30_000, 2, true, 88, 90, true, None),
    ];
    let counter = counter();
    for row in rows {
        let env = env(&counter, row.limit, ArchiveSystemPrompt::All, false);
        let mut cache = with_sink(Arc::new(FakeSink::new(&[(row.label, row.frames)])));
        let mut messages = system_messages(&[&text(row.label, row.tokens)]);
        let mut attachments = Attachments::new();
        if row.user {
            messages.push(message(MessageRole::User, "hello"));
            if row.source_images > 0 {
                let ids: Vec<String> = (0..row.source_images).map(|n| format!("shot{n}")).collect();
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                attach(&mut attachments, 1, &ids);
            }
        }
        let analysis = analyze(
            &env,
            &messages,
            &attachments,
            &facts(messages.len()),
            &mut cache,
            false,
        );
        assert_eq!(analysis.selection.system, row.swapped, "{}", row.label);
        assert_eq!(analysis.reason, row.reason, "{}", row.label);
    }
}

#[test]
fn frames_ride_the_first_user_before_its_content_and_the_prompt_becomes_one_stub() {
    let counter = counter();
    let env = env(&counter, 90, ArchiveSystemPrompt::All, false);
    let mut cache = with_sink(Arc::new(FakeSink::new(&[("sys", 2)])));
    let mut messages = system_messages(&[&text("sys", 20_000), "tail"]);
    messages.push(message(MessageRole::User, "question"));
    let mut attachments = Attachments::new();
    attach(&mut attachments, 2, &["upload"]);
    let analysis = analyze(&env, &messages, &attachments, &facts(3), &mut cache, false);
    let mut content = empty_content();
    apply(&analysis, &mut messages, &mut attachments, &mut content);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, MessageRole::System);
    assert!(!messages[0].content.contains("tail"));
    let parts = content.get(&1).map(Vec::as_slice).unwrap_or_default();
    assert!(matches!(parts[0], MessageContentPart::Text(_)));
    assert_eq!(parts[1], MessageContentPart::Attachment(0));
    assert_eq!(parts[2], MessageContentPart::Attachment(1));
    assert_eq!(parts[3], MessageContentPart::Text("question".into()));
    assert_eq!(parts[4], MessageContentPart::Attachment(2));
    let carried = attachments.get(&1).map(|list| list.len());
    assert_eq!(carried, Some(3));
    assert_eq!(attachments[&1][2].id, "upload");
}

#[test]
fn the_estimator_assumes_a_carrier_for_an_empty_history_and_reports_backstopped_reasons() {
    use crate::coord::turn::request::inline::estimate::{estimate, SystemOutcome};
    let counter = counter();
    let env = env(&counter, 90, ArchiveSystemPrompt::All, false);
    let raw = env.shape.raw_frame_tokens();
    let messages = system_messages(&[&text("sys", 20_000)]);
    let sized = || with_sink(Arc::new(FakeSink::new(&[("sys", 2)])));
    let (flags, none) = (facts(1), Attachments::new());
    let hypothetical = analyze(&env, &messages, &none, &flags, &mut sized(), true);
    assert!(matches!(
        estimate(&hypothetical, raw).system,
        SystemOutcome::Swap { frames: 2, .. }
    ));
    let real = analyze(&env, &messages, &none, &flags, &mut sized(), false);
    assert_eq!(
        estimate(&real, raw).system,
        SystemOutcome::Skipped(SystemReason::NoCarrier)
    );
    // A later screenshot flood makes the cap backstop retract the swap: budget.
    let mut crowded = system_messages(&[&text("sys", 20_000)]);
    crowded.push(message(MessageRole::User, "go"));
    crowded.push(message(MessageRole::User, "shots"));
    let mut attachments = Attachments::new();
    let ids: Vec<String> = (0..89).map(|n| format!("s{n}")).collect();
    attach(
        &mut attachments,
        2,
        &ids.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let analysis = analyze(&env, &crowded, &attachments, &facts(3), &mut sized(), false);
    assert_eq!(
        estimate(&analysis, raw).system,
        SystemOutcome::Skipped(SystemReason::Budget)
    );
}
