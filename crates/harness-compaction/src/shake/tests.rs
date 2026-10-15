use super::*;
use crate::{
    item::{Item, ItemKind, Stop, ToolFacts},
    prune::{skill_protection, ToolMatcher},
};
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};
use harness_tokenizer::Counter;

fn regions(
    items: &[Item<'_>],
    boundary: usize,
    cfg: &ShakeConfig,
    protected: &[ToolMatcher],
) -> Vec<ShakeRegion> {
    detect_regions(items, boundary, cfg, protected, &Counter::new(None)).regions
}
fn estimate_text(text: &str) -> u32 {
    Counter::new(None).count_text(text)
}
fn item(message: &CompletionMessage) -> Item<'_> {
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
        at_ms: None,
        tokens: estimate_text(&message.content),
        ..Item::new(message)
    }
}
#[test]
fn block_detection_and_reverse_splicing_preserve_prose() {
    for (text, expected) in [
        ("head\n```rust\na\n~~~\ntail", 1),
        ("```\na", 0),
        ("<outer>\n<inner>\na\n</inner>\n</outer>", 1),
        ("<Upper>\na\n</Upper>", 0),
        (" <outer>\na\n</outer>", 0),
        ("<outer>\n```\na\n```\n</outer>", 1),
        ("head\n```\na\n```\nmiddle\n~~~\nb\n~~~\ntail", 2),
    ] {
        let message = CompletionMessage::text(MessageRole::Assistant, text);
        let cfg = ShakeConfig {
            protect_tokens: 0,
            min_savings: 0,
            fence_min_tokens: 0,
        };
        let regions = regions(&[item(&message)], 0, &cfg, &[]);
        assert_eq!(regions.len(), expected, "{text}");
        let (_, edits) = plan_edits(&regions, Some("shake.txt"));
        let mut replaced = text.to_owned();
        for edit in edits {
            replaced.replace_range(edit.start..edit.end, &edit.replacement);
        }
        if text.starts_with("head") && expected > 0 {
            assert!(replaced.starts_with("head\n") && replaced.ends_with("\ntail"));
        }
        if expected == 2 {
            assert!(
                replaced.contains("\nmiddle\n")
                    && replaced.contains("(region 1)")
                    && replaced.contains("(region 2)")
            );
        }
    }
}
#[test]
fn tool_regions_honor_protection_useless_bypass_and_estimated_savings() {
    let message = CompletionMessage::text(MessageRole::Tool, "output ".repeat(100));
    for (useless, error, pruned, protected, protect, minimum, count) in [
        (false, false, false, false, 10000, 0, 0),
        (true, false, false, false, 10000, 0, 1),
        (true, true, false, false, 10000, 0, 0),
        (true, false, true, false, 0, 0, 0),
        (true, false, false, true, 0, 0, 0),
        (false, false, false, false, 0, 10000, 0),
    ] {
        let mut entry = item(&message);
        entry.tool.useless = useless;
        entry.tool.is_error = error;
        entry.tool.pruned = pruned;
        entry.image_count = 1;
        let cfg = ShakeConfig {
            protect_tokens: protect,
            min_savings: minimum,
            fence_min_tokens: 400,
        };
        let protectors = if protected {
            vec![ToolMatcher::predicate(|_, _| true)]
        } else {
            Vec::new()
        };
        let regions = regions(&[entry], 0, &cfg, &protectors);
        assert_eq!(regions.len(), count);
        if let Some(region) = regions.first() {
            assert_eq!(region.data().0, 175);
            assert_eq!(
                artifact_text(&regions),
                "### region 1 (tool, ~175 tok)\n\n".to_owned() + &message.content + "\n"
            );
        }
    }
}
#[test]
fn presentation_strings_are_model_and_ui_contracts() {
    assert_eq!(
        placeholder(30, Some("shake.txt"), 2),
        "[shaken ~30 tokens — recover: artifact://shake.txt (region 2)]"
    );
    for (mode, tools, blocks, images, thinking, tokens, expected) in [
        (ShakeMode::Elide, 0, 0, 0, 0, 0, "Nothing to shake."),
        (
            ShakeMode::Elide,
            1,
            2,
            0,
            0,
            10,
            "Shook 1 tool result + 2 blocks (~10 tokens freed).",
        ),
        (
            ShakeMode::Images,
            0,
            0,
            0,
            0,
            0,
            "No images found in this session.",
        ),
        (
            ShakeMode::Images,
            0,
            0,
            1,
            0,
            0,
            "Dropped 1 image from this session.",
        ),
        (
            ShakeMode::Thinking,
            0,
            0,
            0,
            0,
            0,
            "No thinking blocks found in this session.",
        ),
        (
            ShakeMode::Thinking,
            0,
            0,
            0,
            2,
            10,
            "Dropped 2 thinking blocks from this session (~10 tokens freed).",
        ),
        (
            ShakeMode::Thinking,
            0,
            0,
            0,
            1,
            0,
            "Dropped 1 thinking block from this session.",
        ),
    ] {
        assert_eq!(
            summarize_shake(mode, tools, blocks, images, thinking, tokens),
            expected
        );
    }
}
#[test]
fn skill_and_recovery_reads_use_preset_protection() {
    for (name, path, artifact_protection, count) in [
        ("skill", "file", false, 0),
        ("read", "skill://guide", false, 0),
        ("read", "artifact://shake.txt", true, 0),
        ("read", "artifact://shake.txt", false, 1),
        ("read", "file", true, 1),
    ] {
        let mut call = CompletionMessage::text(MessageRole::Assistant, "");
        call.assistant_tool_calls = Some(vec![AssistantToolCall {
            tool_call_id: "id".into(),
            function_name: "read".into(),
            arguments_json: serde_json::json!({"path":path}).to_string(),
        }]);
        let mut result = CompletionMessage::text(MessageRole::Tool, "content".repeat(100));
        result.name = Some(name.into());
        result.tool_call_id = Some("id".into());
        let cfg = ShakeConfig {
            protect_tokens: 0,
            min_savings: 0,
            fence_min_tokens: 400,
        };
        let mut protected = skill_protection();
        if artifact_protection {
            protected.push(ToolMatcher::artifact_recovery());
        }
        assert_eq!(
            regions(&[item(&call), item(&result)], 1, &cfg, &protected).len(),
            count
        );
    }
}
#[test]
fn recent_window_threshold_and_presets_preserve_whole_entries() {
    let messages = [
        CompletionMessage::text(MessageRole::Tool, "x".repeat(400)),
        CompletionMessage::text(MessageRole::Tool, "x".repeat(400)),
        CompletionMessage::text(MessageRole::Tool, "x".repeat(400)),
    ];
    let items: Vec<_> = messages.iter().map(item).collect();
    for (protect, minimum, expected) in
        [(150, 0, vec![0]), (0, 252, vec![0, 1, 2]), (0, 253, vec![])]
    {
        let cfg = ShakeConfig {
            protect_tokens: protect,
            min_savings: minimum,
            fence_min_tokens: 400,
        };
        let regions = regions(&items, 0, &cfg, &[]);
        assert_eq!(
            regions
                .iter()
                .map(|region| match region {
                    ShakeRegion::ToolResult { index, .. } | ShakeRegion::Block { index, .. } =>
                        *index,
                })
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert!(regions(&items, 0, &MANUAL, &[]).is_empty());
    assert_eq!(regions(&items[2..], 0, &RESCUE, &[]).len(), 1);
    let text = CompletionMessage::text(
        MessageRole::Assistant,
        "```\n".to_owned() + &"x".repeat(1592) + "\n```",
    );
    for (floor, count) in [(400, 1), (401, 0)] {
        let cfg = ShakeConfig {
            protect_tokens: 0,
            min_savings: 0,
            fence_min_tokens: floor,
        };
        assert_eq!(regions(&[item(&text)], 0, &cfg, &[]).len(), count);
    }
}
#[test]
fn ordered_text_blocks_are_scanned_separately_with_absolute_offsets() {
    let fence = format!("```\n{}\n```", "x".repeat(40));
    let first = format!("intro\n{fence}\n");
    let second = format!("middle\n{fence}");
    let content = format!("{first}{second}");
    let message = CompletionMessage::text(MessageRole::Assistant, content.clone());
    let parts = [
        crate::item::ItemPart::text(first.clone()),
        crate::item::ItemPart::reasoning("private"),
        crate::item::ItemPart::text(second.clone()),
    ];
    let mut entry = item(&message);
    entry.parts = &parts;
    let cfg = ShakeConfig {
        protect_tokens: 0,
        min_savings: 0,
        fence_min_tokens: 0,
    };
    let found = regions(&[entry], 0, &cfg, &[]);
    let spans: Vec<_> = found
        .iter()
        .map(|region| match region {
            ShakeRegion::Block { start, end, .. } => &content[*start..*end],
            ShakeRegion::ToolResult { .. } => "",
        })
        .collect();
    assert_eq!(spans, [fence.as_str(), fence.as_str()]);
}

#[test]
fn exact_savings_price_the_real_placeholder() {
    let message = CompletionMessage::text(MessageRole::Tool, "output ".repeat(100));
    let counter = Counter::new(None);
    let found = regions(&[item(&message)], 0, &RESCUE, &[]);
    let saved = exact_savings(&found, Some("a.txt"), &counter);
    let placeholder = placeholder(found[0].tokens(), Some("a.txt"), 1);
    assert_eq!(saved, found[0].tokens() - counter.count_text(&placeholder));
    assert!(saved < found[0].tokens());
}
