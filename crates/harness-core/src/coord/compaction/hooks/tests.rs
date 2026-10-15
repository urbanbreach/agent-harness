use super::*;
use serde_json::json;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn custom() -> Value {
    json!({
        "summary": "kept by hook",
        "short_summary": "short",
        "first_kept_seq": 12,
        "tokens_before": 345,
        "details": {"files": ["a"]},
        "preserve_data": {"note": 1},
    })
}

#[test]
fn before_results_cancel_beats_a_compaction_and_incomplete_compactions_are_ignored() {
    let cases = [
        (json!({}), false, false),
        (json!({"compaction": custom()}), false, true),
        (json!({"cancel": true, "compaction": custom()}), true, false),
        (
            json!({"cancel": false, "compaction": custom()}),
            false,
            true,
        ),
        (json!({"cancel": 0}), false, false),
        (json!({"cancel": "yes"}), true, false),
        (
            json!({"compaction": {"summary": "no cut point"}}),
            false,
            false,
        ),
        (
            json!({"compaction": {"summary": " ", "first_kept_seq": 1, "tokens_before": 1}}),
            false,
            false,
        ),
        (json!({"compaction": "text"}), false, false),
    ];
    for (input, cancel, has_compaction) in cases {
        let result = parse_before(&object(input.clone()));
        assert_eq!(
            (result.cancel, result.compaction.is_some()),
            (cancel, has_compaction),
            "{input}"
        );
    }
}

#[test]
fn a_custom_compaction_keeps_every_supplied_field_untouched() {
    let result = parse_before(&object(json!({"compaction": custom()})));
    let compaction = result.compaction.map(|c| {
        (
            c.summary,
            c.short_summary,
            c.first_kept_seq,
            c.tokens_before,
            c.details,
            c.preserve_data.get("note").cloned(),
        )
    });
    assert_eq!(
        compaction,
        Some((
            "kept by hook".to_owned(),
            Some("short".to_owned()),
            12,
            345,
            Some(json!({"files": ["a"]})),
            Some(json!(1)),
        ))
    );
}

#[test]
fn summarizing_results_take_string_context_in_order_and_ignore_other_shapes() {
    let result = parse_summarizing(&object(json!({
        "context": ["first", 7, "second"],
        "prompt": "custom base",
        "preserve_data": {"k": "v"},
    })));
    assert_eq!(result.context, ["first", "second"]);
    assert_eq!(result.prompt.as_deref(), Some("custom base"));
    assert_eq!(result.preserve_data.get("k"), Some(&json!("v")));
    let empty = parse_summarizing(&object(json!({"context": "line", "preserve_data": []})));
    assert_eq!(empty, SummarizingResult::default());
}
