use super::{history, item};
use crate::{
    item::ItemPart,
    prune::{
        call_index, plan_age_prune, plan_incremental_prune, read_complete, read_key,
        skill_protection, tool_facts, AgePruneConfig, IncrementalPruneConfig, PruneKind,
        ResultFacts, ToolMatcher, SUPERSEDED_NOTICE,
    },
};
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use serde_json::json;

fn read_pair(path: &str, name: &str) -> [CompletionMessage; 2] {
    let mut call = CompletionMessage::text(MessageRole::Assistant, "");
    call.assistant_tool_calls = Some(vec![AssistantToolCall {
        tool_call_id: "id".into(),
        function_name: "read".into(),
        arguments_json: json!({"path": path}).to_string(),
    }]);
    let mut result = CompletionMessage::text(MessageRole::Tool, "data ".repeat(100));
    result.name = Some(name.into());
    result.tool_call_id = Some("id".into());
    [call, result]
}

#[test]
fn any_matcher_protects_and_each_kind_matches_what_it_names() {
    for (name, path, internal, matchers, protected) in [
        ("skill", "x", false, vec![ToolMatcher::skill_tool()], true),
        ("bash", "x", false, vec![ToolMatcher::skill_tool()], false),
        ("read", "skill://a", false, skill_protection(), true),
        (
            "read",
            "skill://a",
            false,
            vec![ToolMatcher::skill_tool()],
            false,
        ),
        ("grep", "skill://a", false, skill_protection(), false),
        (
            "read",
            "artifact://a.txt",
            false,
            vec![ToolMatcher::artifact_recovery()],
            true,
        ),
        (
            "read",
            "src/a.rs",
            true,
            vec![ToolMatcher::artifact_recovery()],
            true,
        ),
        (
            "read",
            "src/a.rs",
            false,
            vec![ToolMatcher::artifact_recovery()],
            false,
        ),
        (
            "read",
            "src/a.rs",
            false,
            vec![ToolMatcher::name("bash"), ToolMatcher::name("read")],
            true,
        ),
    ] {
        let messages = read_pair(path, name);
        let mut result = item(&messages[1]);
        result.tool.internal_artifact = internal;
        let items = [item(&messages[0]), result];
        let calls = call_index(&items, 0..2);
        assert_eq!(
            crate::prune::protects(&matchers, &items[1], calls.get("id").copied()),
            protected,
            "{name} {path} {internal}"
        );
    }
}

#[test]
fn calls_outside_the_interval_do_not_pair() {
    let messages = history(&[r#"{"path":"a"}"#, r#"{"path":"b"}"#]);
    let items: Vec<_> = messages.iter().map(item).collect();
    assert_eq!(
        call_index(&items, 2..4).keys().copied().collect::<Vec<_>>(),
        ["1"]
    );
}

#[test]
fn excluded_prefix_call_still_resolves_but_is_never_mutated() {
    // The call is before the boundary; its result is inside the sent region.
    let mut messages = history(&[r#"{"path":"file"}"#, r#"{"path":"file"}"#]);
    messages.remove(3);
    messages.insert(3, {
        let mut result = CompletionMessage::text(MessageRole::Tool, "data ".repeat(100));
        result.name = Some("read".into());
        result.tool_call_id = Some("0".into());
        result
    });
    let mut items: Vec<_> = messages.iter().map(item).collect();
    for entry in &mut items {
        entry.tool.read_complete = Some(true);
    }
    let cfg = IncrementalPruneConfig {
        now_ms: 1,
        supersede_key: Some(&read_key),
        ..IncrementalPruneConfig::default()
    };
    let plan = plan_incremental_prune(&items, 2, &cfg, &[]);
    assert!(plan.edits.iter().all(|edit| edit.index >= 2));
}

#[test]
fn age_pass_supersedes_inside_the_protected_window_only_with_a_key() {
    let messages = history(&[r#"{"path":"file"}"#, r#"{"path":"file"}"#]);
    for (key, expected) in [(true, vec![PruneKind::Superseded]), (false, vec![])] {
        let items: Vec<_> = messages.iter().map(item).collect();
        let cfg = AgePruneConfig {
            protect_tokens: 1_000_000,
            minimum_savings: 0,
            supersede_key: key.then_some(&read_key),
            ..AgePruneConfig::default()
        };
        let plan = plan_age_prune(&items, 0, &cfg, &[]);
        assert_eq!(
            plan.edits.iter().map(|edit| edit.kind).collect::<Vec<_>>(),
            expected
        );
        assert!(plan
            .edits
            .iter()
            .all(|edit| edit.replacement == SUPERSEDED_NOTICE));
        assert_eq!(plan.savings > 0, key);
    }
}

#[test]
fn without_a_completeness_callback_every_success_is_complete() {
    let messages = history(&[
        r#"{"path":"file","offset":2,"limit":5}"#,
        r#"{"path":"file"}"#,
    ]);
    let items: Vec<_> = messages.iter().map(item).collect();
    let cfg = |complete: bool| IncrementalPruneConfig {
        now_ms: 1,
        supersede_key: Some(&read_key),
        supersede_complete: complete.then_some(&crate::prune::read_is_complete),
        ..IncrementalPruneConfig::default()
    };
    // Default: the bare read is complete, so it supersedes the older range read.
    assert_eq!(
        plan_incremental_prune(&items, 0, &cfg(false), &[])
            .edits
            .len(),
        1
    );
    // Session proof: no tool reported completeness, so the range survives.
    assert!(plan_incremental_prune(&items, 0, &cfg(true), &[])
        .edits
        .is_empty());
}

#[test]
fn read_completeness_needs_proof_and_no_truncation() {
    for (structured, complete) in [
        (json!({"totalLines": 3, "truncated": false}), true),
        (json!({"total_lines": 3}), true),
        (json!({"totalLines": 3, "truncated": true}), false),
        (json!({"totalLines": 3, "summary": "short"}), false),
        (
            json!({"totalLines": 3, "truncation": {"truncated": true}}),
            false,
        ),
        (
            json!({"totalLines": 3, "meta": {"limits": {"columnTruncated": true}}}),
            false,
        ),
        (json!({"kind": "image"}), true),
        (json!({"mime": "application/pdf"}), false),
    ] {
        assert_eq!(read_complete(&structured), complete, "{structured}");
    }
}

#[test]
fn facts_never_flag_errors_and_only_reads_carry_completeness() {
    let structured = json!({"totalLines": 1});
    let base = ResultFacts {
        tool: "read",
        is_error: false,
        useless: true,
        source_artifact: true,
        structured: Some(&structured),
    };
    let ok = tool_facts(base);
    assert!(ok.useless && ok.internal_artifact);
    assert_eq!(ok.read_complete, Some(true));
    let failed = tool_facts(ResultFacts {
        is_error: true,
        ..base
    });
    assert!(!failed.useless);
    assert_eq!(failed.read_complete, None);
    assert_eq!(
        tool_facts(ResultFacts {
            tool: "grep",
            ..base
        })
        .read_complete,
        None
    );
}

#[test]
fn ordered_parts_drive_block_counts_for_lookback() {
    // The entry type is exercised through `lookback`; this guards ItemPart wiring.
    let message = CompletionMessage::text(MessageRole::Assistant, "hello");
    let parts = [ItemPart::text("hello")];
    let mut entry = item(&message);
    entry.parts = &parts;
    for (lookback, floor) in [(Some(8), 0), (Some(7), 1)] {
        assert_eq!(crate::prune::lookback_floor(&[entry], 0, lookback), floor);
    }
}
