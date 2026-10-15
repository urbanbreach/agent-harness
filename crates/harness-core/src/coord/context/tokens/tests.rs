use super::*;
use crate::attachment_transport::{AttachmentDimensions, AttachmentMetadata};
use crate::coord::context::CompactionState;
use harness_compaction::{
    tokenizer::{Reader, ReaderApi},
    CompactionMethod,
};
use harness_providers::{CompletionMessage, CompletionUsage};

fn bytes() -> Counter {
    Counter::byte_estimating(None)
}

fn assistant(text: &str) -> Entry {
    let mut entry = Entry::new(
        CompletionMessage::text(MessageRole::Assistant, text),
        1,
        None,
    );
    entry.usage = Some(CompletionUsage {
        total_tokens: 10,
        ..Default::default()
    });
    entry
}

fn state(summary: &str) -> CompactionState {
    CompactionState {
        summary: summary.into(),
        short_summary: None,
        method: CompactionMethod::Archive,
        tokens_before: 0,
        tokens_after: None,
        read_files: Vec::new(),
        modified_files: Vec::new(),
        warning: None,
        archive: None,
        native: None,
        from_hook: false,
        details: None,
        preserve_data: Default::default(),
        commit_seq: 0,
        history_rewrite_at_ms: None,
    }
}

#[test]
fn entry_prices_memoize_until_the_version_bumps() {
    let counter = bytes();
    let mut entry = assistant("abcd");
    assert_eq!(entry_price(&counter, &entry).normal, 1);
    // An edit that forgets the version bump is invisible to the memo: this is why every
    // mutation owner must bump.
    entry.settled_reasoning.push("x".repeat(40));
    assert_eq!(entry_price(&counter, &entry).normal, 1);
    entry.version = entry.version.wrapping_add(1);
    assert_eq!(entry_price(&counter, &entry).normal, 11);
}

#[test]
fn assistants_without_settled_usage_bypass_the_memo() {
    let counter = bytes();
    for stop in [Stop::Aborted, Stop::Error] {
        let mut entry = assistant("abcd");
        entry.stop = stop;
        assert_eq!(entry_price(&counter, &entry).normal, 1);
        entry.settled_reasoning.push("x".repeat(40));
        assert_eq!(entry_price(&counter, &entry).normal, 11);
    }
    let mut unsettled = assistant("abcd");
    unsettled.usage = None;
    assert_eq!(entry_price(&counter, &unsettled).normal, 1);
    unsettled.settled_reasoning.push("x".repeat(40));
    assert_eq!(entry_price(&counter, &unsettled).normal, 11);
}

#[test]
fn clones_start_with_an_independent_memo() {
    let counter = bytes();
    let entry = assistant("abcd");
    entry_price(&counter, &entry);
    let mut copy = entry.clone();
    copy.settled_reasoning.push("x".repeat(40));
    assert_eq!(entry_price(&counter, &copy).normal, 11);
    assert_eq!(entry_price(&counter, &entry).normal, 1);
}

#[test]
fn summaries_price_the_raw_summary_blocks_and_frames_not_the_wrapped_text() {
    let counter = Counter::byte_estimating(Some(Reader {
        api: ReaderApi::Anthropic,
        provider: "anthropic".into(),
        model_id: "claude-opus-4-7".into(),
        official_endpoint: true,
        prefix_binding: false,
        filter_reasoning_history: false,
    }));
    let mut plain = state("abcdefgh");
    let wrapped = super::super::summary_entry(plain.clone());
    assert_eq!(entry_price(&counter, &wrapped).normal, 2);
    plain.archive = Some(crate::event::ArchiveRecord {
        text_head: "abcd".into(),
        text_tail: "abcd".into(),
        frames: vec![crate::event::ArchiveFrameRecord {
            width: 800,
            height: 600,
            ..Default::default()
        }],
        ..Default::default()
    });
    let archived = super::super::summary_entry(plain);
    assert_eq!(entry_price(&counter, &archived).normal, 2 + 1 + 1 + 29 * 22);
}

#[test]
fn items_carry_both_estimate_domains_and_priced_images() {
    let counter = Counter::byte_estimating(Some(Reader {
        api: ReaderApi::Anthropic,
        provider: "anthropic".into(),
        model_id: "claude-opus-4-7".into(),
        official_endpoint: true,
        prefix_binding: false,
        filter_reasoning_history: false,
    }));
    let mut context = Context::new("system");
    context.push(CompletionMessage::text(MessageRole::User, ""), 1, None);
    context.entries[1].attachments.push(AttachmentMetadata::new(
        "i",
        "image/png",
        10,
        Some(AttachmentDimensions::new(1000, 1000)),
        "blob",
    ));
    let items = context.items_with(&counter);
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].tokens, items[0].tokens_floor), (1296, 1296));
    assert_eq!(items[0].image_count, 1);
    assert_eq!(context.tokens_with(&counter), 2 + 1296);
}
