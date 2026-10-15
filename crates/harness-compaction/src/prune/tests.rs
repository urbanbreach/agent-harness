use super::*;
use crate::item::{ItemKind, Stop, ToolFacts};
use harness_tokenizer::Counter;

fn estimate_text(text: &str) -> u32 {
    Counter::new(None).count_text(text)
}
use harness_providers::{CompletionMessage, MessageRole};
pub(crate) fn item(message: &CompletionMessage) -> Item<'_> {
    Item {
        message,
        kind: ItemKind::Message,
        seq: 0,
        turn: None,
        reasoning: &[],
        tool: ToolFacts::default(),
        image_count: 0,
        image_tokens: 0,
        usage: None,
        stop: Stop::Complete,
        at_ms: Some(0),
        tokens: estimate_text(&message.content),
        ..Item::new(message)
    }
}
fn history(arguments: &[&str]) -> Vec<CompletionMessage> {
    arguments
        .iter()
        .enumerate()
        .flat_map(|(index, arguments)| {
            let id = index.to_string();
            let mut call = CompletionMessage::text(MessageRole::Assistant, "");
            call.assistant_tool_calls = Some(vec![AssistantToolCall {
                tool_call_id: id.clone(),
                function_name: "read".into(),
                arguments_json: (*arguments).into(),
            }]);
            let mut result = CompletionMessage::text(MessageRole::Tool, "data ".repeat(100));
            result.name = Some("read".into());
            result.tool_call_id = Some(id);
            [call, result]
        })
        .collect()
}
#[test]
fn supersession_preserves_errors_completeness_and_selector_direction() {
    let bare = r#"{"path":"file"}"#;
    let range = r#"{"path":"file","offset":2,"limit":5}"#;
    for (args, errors, complete, expected) in [
        (
            vec![bare, bare],
            vec![false, false],
            vec![true, true],
            vec![1],
        ),
        (
            vec![range, bare],
            vec![false, false],
            vec![true, false],
            vec![],
        ),
        (
            vec![range, bare],
            vec![false, false],
            vec![true, true],
            vec![1],
        ),
        (
            vec![bare, range],
            vec![false, false],
            vec![true, true],
            vec![],
        ),
        (
            vec![range, range],
            vec![false, false],
            vec![true, false],
            vec![1],
        ),
        (
            vec![range, bare, bare],
            vec![false, false, false],
            vec![true, true, false],
            vec![3],
        ),
        (
            vec![bare, bare, bare],
            vec![false, true, true],
            vec![true, false, false],
            vec![3],
        ),
        (
            vec![range, bare],
            vec![true, false],
            vec![false, false],
            vec![1],
        ),
    ] {
        let messages = history(&args);
        let mut items: Vec<_> = messages.iter().map(item).collect();
        for (index, (&error, &complete)) in errors.iter().zip(&complete).enumerate() {
            items[index * 2 + 1].tool.is_error = error;
            items[index * 2 + 1].tool.read_complete = Some(complete);
        }
        let cfg = IncrementalPruneConfig {
            prune_useless: true,
            suffix_token_limit: 8000,
            idle_flush_ms: 1800000,
            now_ms: 1,
            cache_lookback: None,
            supersede_key: Some(&read_key),
            supersede_complete: Some(&read_is_complete),
        };
        let edits = plan_incremental_prune(&items, 0, &cfg, &[]).edits;
        assert_eq!(
            edits.iter().map(|edit| edit.index).collect::<Vec<_>>(),
            expected
        );
        assert!(edits
            .iter()
            .all(|edit| edit.replacement == SUPERSEDED_NOTICE));
    }
}
#[test]
fn cache_timing_boundary_and_latest_pruned_read() {
    let messages = history(&[
        r#"{"path":"file"}"#,
        r#"{"path":"file"}"#,
        r#"{"path":"file"}"#,
    ]);
    for (limit, now, boundary, pruned, expected) in [
        (0, 1, 0, false, vec![]),
        (0, 1800000, 0, false, vec![1, 3]),
        (0, 1800000, 2, false, vec![3]),
        (8000, 1, 0, true, vec![1]),
    ] {
        let mut items: Vec<_> = messages.iter().map(item).collect();
        for entry in &mut items {
            entry.tool.read_complete = Some(true);
        }
        items[5].tool.pruned = pruned;
        let cfg = IncrementalPruneConfig {
            prune_useless: false,
            suffix_token_limit: limit,
            idle_flush_ms: 1800000,
            now_ms: now,
            cache_lookback: None,
            supersede_key: Some(&read_key),
            supersede_complete: Some(&read_is_complete),
        };
        assert_eq!(
            plan_incremental_prune(&items, boundary, &cfg, &[])
                .edits
                .iter()
                .map(|edit| edit.index)
                .collect::<Vec<_>>(),
            expected
        );
    }
}
#[test]
fn age_floor_useless_bypass_and_savings_gate() {
    for (text, useless, error, protect, minimum, expected) in [
        ("ok", false, false, 0, 0, None),
        ("No matches found", true, false, 1000, 0, None),
        (
            "long output ",
            true,
            false,
            1000,
            0,
            Some(PruneKind::Useless),
        ),
        ("long output ", true, true, 1000, 0, None),
        ("long output ", false, false, 0, 0, Some(PruneKind::Aged)),
        ("long output ", true, false, 0, 10000, None),
    ] {
        let message = CompletionMessage::text(
            MessageRole::Tool,
            if text == "long output " {
                text.repeat(100)
            } else {
                text.into()
            },
        );
        let mut entry = item(&message);
        entry.tool.useless = useless;
        entry.tool.is_error = error;
        let cfg = AgePruneConfig {
            protect_tokens: protect,
            minimum_savings: minimum,
            prune_useless: true,
            cache_warm_suffix: None,
            cache_lookback: None,
            ..AgePruneConfig::default()
        };
        let plan = plan_age_prune(&[entry], 0, &cfg, &[]);
        assert_eq!(plan.edits.first().map(|edit| edit.kind), expected);
    }
}
#[test]
fn cache_positions_collapse_parallel_batches_and_idle_overrides_lookback() {
    let mut messages = history(&[r#"{"path":"file"}"#, r#"{"path":"file"}"#]);
    for _ in 0..8 {
        messages.push(CompletionMessage::text(MessageRole::Tool, "small"));
    }
    for (lookback, cold, expected) in [
        (Some(10), false, 0),
        (Some(11), false, 1),
        (Some(1), true, 1),
        (None, false, 1),
    ] {
        let mut items: Vec<_> = messages.iter().map(item).collect();
        for entry in &mut items {
            entry.tool.read_complete = Some(true);
        }
        let cfg = IncrementalPruneConfig {
            prune_useless: false,
            suffix_token_limit: 8000,
            idle_flush_ms: 1800000,
            now_ms: if cold { 1800000 } else { 1 },
            cache_lookback: lookback,
            supersede_key: Some(&read_key),
            supersede_complete: Some(&read_is_complete),
        };
        assert_eq!(
            plan_incremental_prune(&items, 0, &cfg, &[]).edits.len(),
            expected
        );
    }
}
#[test]
fn age_windows_count_only_outputs_and_cache_suffix_counts_messages() {
    let messages = [
        CompletionMessage::text(MessageRole::Tool, "old ".repeat(100)),
        CompletionMessage::text(MessageRole::User, "tail ".repeat(10000)),
        CompletionMessage::text(MessageRole::Tool, "new ".repeat(100)),
    ];
    for (protect, warm, minimum, expected) in [
        (100, None, 0, vec![0]),
        (101, None, 0, vec![]),
        (0, Some(8000), 0, vec![2]),
        (0, None, 10000, vec![]),
    ] {
        let items: Vec<_> = messages.iter().map(item).collect();
        let cfg = AgePruneConfig {
            protect_tokens: protect,
            minimum_savings: minimum,
            prune_useless: false,
            cache_warm_suffix: warm,
            cache_lookback: Some(1),
            ..AgePruneConfig::default()
        };
        let cfg = AgePruneConfig {
            cache_lookback: None,
            ..cfg
        };
        assert_eq!(
            plan_age_prune(&items, 0, &cfg, &[])
                .edits
                .iter()
                .map(|edit| edit.index)
                .collect::<Vec<_>>(),
            expected
        );
    }
}
#[test]
fn incremental_useless_respects_error_protection_size_and_supersede_priority() {
    let messages = history(&[r#"{"path":"file"}"#, r#"{"path":"file"}"#]);
    for (enabled, error, protected, expected) in [
        (
            true,
            false,
            false,
            vec![PruneKind::Superseded, PruneKind::Useless],
        ),
        (false, false, false, vec![PruneKind::Superseded]),
        (true, true, false, vec![PruneKind::Superseded]),
        (true, false, true, vec![]),
    ] {
        let protectors = if protected {
            vec![ToolMatcher::predicate(|_, _| true)]
        } else {
            Vec::new()
        };
        let mut items: Vec<_> = messages.iter().map(item).collect();
        for entry in &mut items {
            entry.tool.read_complete = Some(true);
            entry.tool.useless = true;
            entry.tool.is_error = error;
        }
        let cfg = IncrementalPruneConfig {
            prune_useless: enabled,
            suffix_token_limit: 8000,
            idle_flush_ms: 1800000,
            now_ms: 1,
            cache_lookback: None,
            supersede_key: Some(&read_key),
            supersede_complete: Some(&read_is_complete),
        };
        assert_eq!(
            plan_incremental_prune(&items, 0, &cfg, &protectors)
                .edits
                .iter()
                .map(|edit| edit.kind)
                .collect::<Vec<_>>(),
            expected
        );
    }
}
mod protection;
mod windows;
