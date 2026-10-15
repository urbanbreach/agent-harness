//! Partition provider history without including the previous summary or pending turn.
use crate::{
    cut::find_cut_point,
    files::{collect_file_ops, merge_previous, FileOps},
    tokens::{context_tokens, prompt_tokens},
    Item, ItemKind, Stop,
};
use serde_json::Value;
use std::{collections::BTreeMap, ops::Range};
#[derive(Debug, Clone)]
pub struct Preparation {
    pub summary_index: Option<usize>,
    pub history: Range<usize>,
    pub turn_prefix: Range<usize>,
    pub recent: Range<usize>,
    pub is_split: bool,
    pub tokens_before: u32,
    pub keep_recent_tokens: u32,
    pub file_ops: FileOps,
    pub previous_summary: Option<String>,
    /// When the previous summary was committed, in Unix milliseconds.
    pub previous_summary_at_ms: Option<u64>,
    /// Opaque details the previous summary carried; hooks may extend them.
    pub previous_details: Option<Value>,
    /// Hook-owned data the previous summary preserved.
    pub previous_preserve: BTreeMap<String, Value>,
    /// Journal sequence of the newest original entry in the compactable region.
    pub last_original_seq: u64,
}
pub struct PrepareInput<'a> {
    pub items: &'a [Item<'a>],
    pub pending_from: Option<usize>,
    pub keep_recent_tokens: u32,
    pub previous_summary: Option<&'a str>,
    pub previous_files: Option<(&'a [String], &'a [String])>,
    /// Commit time of the previous summary; defaults to the summary item's own time.
    pub previous_summary_at_ms: Option<u64>,
    pub previous_details: Option<&'a Value>,
    pub previous_preserve: Option<&'a BTreeMap<String, Value>>,
}
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Ratio only reduces a u32 budget"
)]
pub fn prepare(input: &PrepareInput<'_>) -> Option<Preparation> {
    let items = input.items;
    let summary_index = items
        .first()
        .filter(|item| item.kind == ItemKind::Summary)
        .map(|_| 0);
    let start = usize::from(summary_index.is_some());
    let end = input.pending_from.unwrap_or(items.len()).min(items.len());
    if end <= start {
        return None;
    }
    // An original entry is one that is neither a summary nor kept over from before the
    // active summary. Without one there is nothing new to compact.
    let last_original_seq = items[start..end]
        .iter()
        .rev()
        .find(|item| item.kind != ItemKind::Summary && !item.retained)
        .map(|item| item.seq)?;
    let usage = items[..end]
        .iter()
        .rev()
        .find(|item| {
            item.is_assistant()
                && !matches!(item.stop, Stop::Error | Stop::Aborted)
                && item.usage.is_some()
        })
        .and_then(|item| item.usage);
    // `tokens_before` is the newest assistant usage anywhere on the supplied path, the
    // not-yet-sent region included; the keep-budget ratio only trusts usage inside the range.
    let newest = items
        .iter()
        .rev()
        .find(|item| {
            item.is_assistant()
                && !matches!(item.stop, Stop::Error | Stop::Aborted)
                && item.usage.is_some()
        })
        .and_then(|item| item.usage);
    let tokens_before = newest.map_or(0, context_tokens);
    let estimated = items[start..end]
        .iter()
        .fold(0u32, |sum, item| sum.saturating_add(item.tokens));
    let ratio = usage.map_or(0.0, |usage| {
        if estimated == 0 {
            0.0
        } else {
            f64::from(prompt_tokens(usage)) / f64::from(estimated)
        }
    });
    let keep_recent_tokens = if ratio.is_finite() && ratio > 1.0 {
        (f64::from(input.keep_recent_tokens) / ratio)
            .floor()
            .max(1.0) as u32
    } else {
        input.keep_recent_tokens
    };
    let cut = find_cut_point(items, start, end, keep_recent_tokens);
    // An unmigrated entry (sequence 0) cannot be recorded as the first kept entry.
    if items.get(cut.first_kept).is_none_or(|item| item.seq == 0) {
        return None;
    }
    let history_end = cut.turn_start.unwrap_or(cut.first_kept);
    let history = start..history_end;
    let turn_prefix = history_end..cut.first_kept;
    if history.is_empty() && turn_prefix.is_empty() {
        return None;
    }
    let mut file_ops = FileOps::default();
    // Files recorded by a hook-produced summary are not built-in details; do not merge them.
    let hook_summary = summary_index.is_some_and(|index| items[index].from_hook);
    if let Some((read, modified)) = input.previous_files.filter(|_| !hook_summary) {
        merge_previous(&mut file_ops, read, modified);
    }
    collect_file_ops(&items[start..cut.first_kept], &mut file_ops);
    Some(Preparation {
        summary_index,
        history,
        turn_prefix,
        recent: cut.first_kept..items.len(),
        is_split: cut.is_split,
        tokens_before,
        keep_recent_tokens,
        file_ops,
        previous_summary: input
            .previous_summary
            .map(str::to_owned)
            .or_else(|| summary_index.map(|index| items[index].text().to_owned())),
        previous_summary_at_ms: input
            .previous_summary_at_ms
            .or_else(|| summary_index.and_then(|index| items[index].at_ms)),
        previous_details: input.previous_details.cloned(),
        previous_preserve: input.previous_preserve.cloned().unwrap_or_default(),
        last_original_seq,
    })
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use harness_providers::{CompletionMessage, CompletionUsage, MessageRole};
    pub(crate) fn item(message: &CompletionMessage) -> Item<'_> {
        Item {
            message,
            kind: ItemKind::Message,
            seq: 1,
            turn: None,
            reasoning: &[],
            tool: crate::ToolFacts::default(),
            image_count: 0,
            image_tokens: 0,
            usage: None,
            stop: Stop::Complete,
            at_ms: None,
            tokens: u32::try_from(message.content.len().div_ceil(4)).unwrap_or(u32::MAX),
            ..Item::new(message)
        }
    }
    pub(crate) fn input<'a>(items: &'a [Item<'a>]) -> PrepareInput<'a> {
        PrepareInput {
            items,
            pending_from: None,
            keep_recent_tokens: 20_000,
            previous_summary: None,
            previous_files: None,
            previous_summary_at_ms: None,
            previous_details: None,
            previous_preserve: None,
        }
    }
    #[test]
    fn cut_partition_parity() {
        for kind in [ItemKind::Note, ItemKind::BranchSummary, ItemKind::Message] {
            let messages = [
                CompletionMessage::text(MessageRole::User, "x".repeat(100_000)),
                CompletionMessage::text(MessageRole::Assistant, "small answer"),
            ];
            let mut items = messages.iter().map(item).collect::<Vec<_>>();
            items[0].kind = kind;
            let result = prepare(&input(&items));
            assert_eq!(
                result.map(|p| (p.history, p.turn_prefix, p.recent, p.is_split)),
                Some((0..0, 0..1, 1..2, true))
            );
        }
        let messages = [
            CompletionMessage::text(MessageRole::Assistant, "x".repeat(100_000)),
            CompletionMessage::text(MessageRole::User, "question"),
            CompletionMessage::text(MessageRole::Assistant, "answer"),
        ];
        let items = messages.iter().map(item).collect::<Vec<_>>();
        let result = prepare(&input(&items));
        assert_eq!(
            result.map(|p| (p.history, p.turn_prefix, p.recent, p.is_split)),
            Some((0..1, 1..1, 1..3, false))
        );
    }
    #[test]
    fn ratio_summary_and_pending_are_preserved() {
        let messages = [
            CompletionMessage::text(MessageRole::User, "summary"),
            CompletionMessage::text(MessageRole::User, "old"),
            CompletionMessage::text(MessageRole::Assistant, "answer"),
            CompletionMessage::text(MessageRole::User, "pending"),
        ];
        for (estimated, prompt, budget) in [
            (100_000, 200_000, 10_000),
            (100_000, 50_000, 20_000),
            (0, 200_000, 20_000),
        ] {
            let usage = CompletionUsage {
                prompt_tokens: prompt,
                completion_tokens: 10,
                total_tokens: prompt + 10,
                ..Default::default()
            };
            let mut items = messages.iter().map(item).collect::<Vec<_>>();
            items[0].kind = ItemKind::Summary;
            items[1].tokens = estimated;
            items[2].tokens = 0;
            items[2].usage = Some(&usage);
            let result = prepare(&PrepareInput {
                items: &items,
                pending_from: Some(3),
                previous_summary: Some("raw summary"),
                ..input(&items)
            });
            if estimated == 0 {
                assert!(result.is_none());
            } else {
                assert_eq!(
                    result.map(|p| (
                        p.summary_index,
                        p.previous_summary,
                        p.history,
                        p.turn_prefix,
                        p.recent,
                        p.keep_recent_tokens,
                        p.tokens_before
                    )),
                    Some((
                        Some(0),
                        Some("raw summary".into()),
                        1..1,
                        1..2,
                        2..4,
                        budget,
                        prompt + 10
                    ))
                );
            }
        }
    }
    #[test]
    fn tokens_before_reads_usage_from_the_not_yet_sent_region() {
        let messages = [
            CompletionMessage::text(MessageRole::User, "old"),
            CompletionMessage::text(MessageRole::Assistant, "answer"),
            CompletionMessage::text(MessageRole::User, "pending"),
            CompletionMessage::text(MessageRole::Assistant, "later answer"),
        ];
        let usage = CompletionUsage {
            prompt_tokens: 900,
            completion_tokens: 100,
            total_tokens: 1_000,
            ..Default::default()
        };
        let mut items = messages.iter().map(item).collect::<Vec<_>>();
        items[0].tokens = 30_000;
        items[3].usage = Some(&usage);
        let prepared = prepare(&PrepareInput {
            items: &items,
            pending_from: Some(2),
            keep_recent_tokens: 10,
            previous_summary: None,
            previous_files: None,
            previous_summary_at_ms: None,
            previous_details: None,
            previous_preserve: None,
        });
        assert_eq!(prepared.map(|p| p.tokens_before), Some(1_000));
    }
    #[test]
    fn files_follow_discarded_history_not_recent_or_pending() {
        use harness_providers::AssistantToolCall;
        let mut messages = [
            CompletionMessage::text(MessageRole::Assistant, "old"),
            CompletionMessage::text(MessageRole::Assistant, "recent"),
            CompletionMessage::text(MessageRole::Assistant, "pending"),
        ];
        for (message, path) in messages
            .iter_mut()
            .zip(["old.rs", "recent.rs", "pending.rs"])
        {
            message.assistant_tool_calls = Some(vec![AssistantToolCall {
                tool_call_id: String::new(),
                function_name: "read".into(),
                arguments_json: serde_json::json!({"path":path}).to_string(),
            }]);
        }
        let mut items = messages.iter().map(item).collect::<Vec<_>>();
        items[0].tokens = 30_000;
        let reads = vec!["prior.rs:2-4".into()];
        let modified = vec!["changed.rs".into()];
        let prepared = prepare(&PrepareInput {
            pending_from: Some(2),
            keep_recent_tokens: 10,
            previous_files: Some((&reads, &modified)),
            ..input(&items)
        });
        assert_eq!(
            prepared.map(|value| crate::files::file_lists(&value.file_ops)),
            Some((
                vec!["old.rs".into(), "prior.rs".into()],
                vec!["changed.rs".into()]
            ))
        );
    }
    fn read_call(message: &mut CompletionMessage, path: &str) {
        use harness_providers::AssistantToolCall;
        message.assistant_tool_calls = Some(vec![AssistantToolCall {
            tool_call_id: String::new(),
            function_name: "read".into(),
            arguments_json: serde_json::json!({"path":path}).to_string(),
        }]);
    }
    #[test]
    fn preparation_requires_original_entries_and_migrated_cut() {
        // (retained flags of items 1..=3, cut item seq, expected last_original_seq)
        for (name, retained, cut_seq, expected) in [
            ("only retained originals", [true, true, true], 3, None),
            (
                "new original after retained",
                [true, true, false],
                3,
                Some(3),
            ),
            ("all new", [false, false, false], 3, Some(3)),
            ("unmigrated cut item", [false, false, false], 0, None),
        ] {
            let messages = [
                CompletionMessage::text(MessageRole::User, "summary"),
                CompletionMessage::text(MessageRole::User, "question"),
                CompletionMessage::text(MessageRole::Assistant, "answer"),
                CompletionMessage::text(MessageRole::Assistant, "newest"),
            ];
            let mut items = messages.iter().map(item).collect::<Vec<_>>();
            items[0].kind = ItemKind::Summary;
            for ((entry, flag), seq) in items[1..].iter_mut().zip(retained).zip(1u64..) {
                entry.retained = flag;
                entry.seq = seq;
                entry.tokens = 30_000;
            }
            // The cut keeps only the newest group, so it lands on the last assistant.
            items[3].seq = cut_seq;
            let prepared = prepare(&PrepareInput {
                keep_recent_tokens: 10,
                ..input(&items)
            });
            assert_eq!(
                prepared.map(|value| value.last_original_seq),
                expected,
                "{name}"
            );
        }
    }
    #[test]
    fn hook_summary_skips_prior_files_and_previous_state_is_carried() {
        let mut messages = [
            CompletionMessage::text(MessageRole::User, "summary"),
            CompletionMessage::text(MessageRole::Assistant, "old"),
            CompletionMessage::text(MessageRole::Assistant, "recent"),
        ];
        read_call(&mut messages[1], "old.rs");
        let details = serde_json::json!({"custom": 1});
        let preserve = BTreeMap::from([("hook".to_owned(), serde_json::json!("data"))]);
        let reads = vec!["prior.rs".to_owned()];
        let modified = vec!["changed.rs".to_owned()];
        for (name, from_hook, summary_at, input_at, expected_reads, expected_at) in [
            (
                "builtin",
                false,
                Some(7),
                None,
                vec!["old.rs", "prior.rs"],
                Some(7),
            ),
            ("hook", true, Some(7), None, vec!["old.rs"], Some(7)),
            (
                "explicit time wins",
                true,
                Some(7),
                Some(9),
                vec!["old.rs"],
                Some(9),
            ),
            (
                "untimed",
                false,
                None,
                None,
                vec!["old.rs", "prior.rs"],
                None,
            ),
        ] {
            let mut items = messages.iter().map(item).collect::<Vec<_>>();
            items[0].kind = ItemKind::Summary;
            items[0].from_hook = from_hook;
            items[0].at_ms = summary_at;
            items[1].tokens = 30_000;
            let prepared = prepare(&PrepareInput {
                keep_recent_tokens: 10,
                previous_files: Some((&reads, &modified)),
                previous_summary_at_ms: input_at,
                previous_details: Some(&details),
                previous_preserve: Some(&preserve),
                ..input(&items)
            });
            let Some(prepared) = prepared else {
                panic!("{name}: expected a preparation");
            };
            assert_eq!(
                crate::files::file_lists(&prepared.file_ops).0,
                expected_reads,
                "{name}"
            );
            assert_eq!(prepared.previous_summary_at_ms, expected_at, "{name}");
            assert_eq!(prepared.previous_details.as_ref(), Some(&details), "{name}");
            assert_eq!(prepared.previous_preserve, preserve, "{name}");
        }
    }
}
