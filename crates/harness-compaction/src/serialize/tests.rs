use super::*;
use crate::{ItemKind, ItemPart, Stop, ToolFacts};
use harness_providers::{AssistantToolCall, CompletionMessage, MessageRole};

mod dialects;
mod lookup;
mod values;

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
        at_ms: None,
        tokens: 0,
        ..Item::new(message)
    }
}

/// One history message with its settled reasoning.
pub(super) struct Turn {
    message: CompletionMessage,
    reasoning: Vec<String>,
    useless: bool,
}

pub(super) fn user(text: &str) -> Turn {
    turn(CompletionMessage::text(MessageRole::User, text))
}

pub(super) fn developer(text: &str) -> Turn {
    turn(CompletionMessage::text(MessageRole::System, text))
}

/// An assistant with `(id, name, arguments JSON)` calls.
pub(super) fn assistant(text: &str, reasoning: &[&str], calls: &[(&str, &str, &str)]) -> Turn {
    let mut message = CompletionMessage::text(MessageRole::Assistant, text);
    if !calls.is_empty() {
        message.assistant_tool_calls = Some(
            calls
                .iter()
                .map(|(id, name, arguments)| AssistantToolCall {
                    tool_call_id: (*id).into(),
                    function_name: (*name).into(),
                    arguments_json: (*arguments).into(),
                })
                .collect(),
        );
    }
    Turn {
        reasoning: reasoning.iter().map(|text| (*text).into()).collect(),
        ..turn(message)
    }
}

pub(super) fn result(id: &str, name: &str, text: &str) -> Turn {
    let mut message = CompletionMessage::text(MessageRole::Tool, text);
    message.tool_call_id = Some(id.into());
    message.name = Some(name.into());
    turn(message)
}

pub(super) fn useless(mut turn: Turn) -> Turn {
    turn.useless = true;
    turn
}

fn turn(message: CompletionMessage) -> Turn {
    Turn {
        message,
        reasoning: Vec::new(),
        useless: false,
    }
}

fn items(turns: &[Turn]) -> Vec<Item<'_>> {
    turns
        .iter()
        .map(|turn| {
            let mut view = item(&turn.message);
            view.reasoning = &turn.reasoning;
            view.tool.useless = turn.useless;
            view
        })
        .collect()
}

/// Summary serialization: neutralized and boundary escaped.
pub(super) fn summary(turns: &[Turn], dialect: Dialect) -> String {
    serialize_for_summary(&items(turns), dialect)
}

/// Native serialization: raw control markers and no boundary escaping.
pub(super) fn native(turns: &[Turn], dialect: Dialect) -> String {
    render(&items(turns), dialect)
}

#[test]
fn useless_pairs_are_global_but_errors_survive_in_every_dialect() {
    let call = ("id", "search", r#"{"z":"value","a":2}"#);
    for dialect in ALL {
        for is_error in [false, true] {
            let turns = [
                assistant("answer", &[], &[call]),
                result("id", "search", r#"{"text":"raw","data":{}}"#),
            ];
            let mut views = items(&turns);
            views[1].tool.useless = true;
            views[1].tool.is_error = is_error;
            let rendered = serialize_for_summary(&views, dialect);
            assert_eq!(rendered.contains("search"), is_error, "{dialect:?}");
            assert_eq!(rendered.contains("raw"), is_error, "{dialect:?}");
        }
    }
}

pub(super) const ALL: [Dialect; 12] = [
    Dialect::Default,
    Dialect::Xml,
    Dialect::Minimax,
    Dialect::Anthropic,
    Dialect::Harmony,
    Dialect::Hermes,
    Dialect::Qwen3,
    Dialect::Gemini,
    Dialect::Gemma,
    Dialect::Glm,
    Dialect::DeepSeek,
    Dialect::Kimi,
];

#[test]
fn default_transcript_labels_thinking_text_and_calls() {
    let turns = [
        user("hi"),
        developer("ignored"),
        assistant(
            "answer",
            &["first", "second"],
            &[("a", "search", r#"{"z":"value","a":2}"#)],
        ),
        result("a", "search", "found"),
    ];
    assert_eq!(
        summary(&turns, Dialect::Default),
        "[User]: hi\n\n[Think]: first\nsecond\n\n[Assistant]: answer\n\n[Tool Call]: search(z=\"value\", a=2)\n\n[Tool Result]: found"
    );
}

#[test]
fn native_user_images_render_their_mime_and_default_stays_bare() {
    use crate::{ImageMeta, ImageOrigin};
    let message = CompletionMessage::text(MessageRole::User, "look ");
    let images = [ImageMeta {
        mime: "image/png".into(),
        width: None,
        height: None,
        detail: None,
        origin: ImageOrigin::User,
    }];
    let with = |count| Item {
        images: &images,
        image_count: count,
        ..item(&message)
    };
    // A counted image with no metadata keeps the bare placeholder.
    assert_eq!(
        serialize_for_summary(&[with(2)], Dialect::Anthropic),
        "\n\nHuman: look [Image: image/png][Image]"
    );
    assert_eq!(
        serialize_for_summary(&[with(1)], Dialect::Default),
        "[User]: look "
    );
}

#[test]
fn ordered_parts_drive_blocks_and_flat_messages_are_unchanged() {
    let message = CompletionMessage::text(MessageRole::Assistant, "");
    let flat = CompletionMessage::text(MessageRole::Assistant, "flat");
    let (two, empty) = (
        [ItemPart::text("A"), ItemPart::text("B")],
        [ItemPart::text("")],
    );
    let mixed = [
        ItemPart::reasoning("t1"),
        ItemPart::RedactedReasoning,
        ItemPart::text("A"),
        ItemPart::reasoning("t2"),
    ];
    let fallback = [ItemPart::text("A")];
    let reasoning = ["r".to_owned()];
    let only_redacted = [ItemPart::RedactedReasoning];
    let none: [ItemPart<'static>; 0] = [];
    let cases: [(
        &CompletionMessage,
        &[ItemPart<'_>],
        &[String],
        Dialect,
        &str,
    ); 10] = [
        (&message, &two, &[], Dialect::Default, "[Assistant]: A\nB"),
        (&message, &two, &[], Dialect::Anthropic, "\n\nAssistant: AB"),
        (&message, &empty, &[], Dialect::Default, "[Assistant]: "),
        (
            &message,
            &mixed,
            &[],
            Dialect::Default,
            "[Think]: t1\nt2\n\n[Assistant]: A",
        ),
        (
            &message,
            &mixed,
            &[],
            Dialect::Anthropic,
            "\n\nAssistant: A",
        ),
        (
            &message,
            &fallback,
            &reasoning,
            Dialect::Default,
            "[Think]: r\n\n[Assistant]: A",
        ),
        (&message, &only_redacted, &[], Dialect::Default, ""),
        (&flat, &none, &[], Dialect::Default, "[Assistant]: flat"),
        (&flat, &none, &[], Dialect::Anthropic, "\n\nAssistant: flat"),
        (&message, &none, &[], Dialect::Default, ""),
    ];
    for (message, parts, reasoning, dialect, expected) in cases {
        let view = Item {
            parts,
            reasoning,
            ..item(message)
        };
        assert_eq!(
            serialize_for_summary(&[view], dialect),
            expected,
            "{parts:?}"
        );
    }
}

#[test]
fn anthropic_drops_thinking_and_assistants_left_empty() {
    let turns = [
        assistant("answer", &["private"], &[]),
        assistant("", &["only thinking"], &[]),
    ];
    assert_eq!(summary(&turns, Dialect::Anthropic), "\n\nAssistant: answer");
    assert_eq!(
        summary(&turns, Dialect::Xml),
        "\n\nAssistant: <thinking>\nprivate\n</thinking>answer\n\nAssistant: <thinking>\nonly thinking\n</thinking>"
    );
}

#[test]
fn tool_truncation_counts_utf16_units_and_boundary_escape_is_narrow() {
    for (text, expected) in [
        ("x".repeat(2000), "x".repeat(2000)),
        (
            "x".repeat(2001),
            format!("{}\n\n[... 1 more characters truncated]", "x".repeat(2000)),
        ),
        (
            "😀".repeat(1001),
            format!("{}\n\n[... 2 more characters truncated]", "😀".repeat(1000)),
        ),
        (
            format!("x{}", "😀".repeat(1000)),
            format!("x{}\n\n[... 1 more characters truncated]", "😀".repeat(999)),
        ),
    ] {
        assert_eq!(
            summary(&[result("a", "read", &text)], Dialect::Default),
            format!("[Tool Result]: {expected}")
        );
    }
    for (text, expected) in [
        ("</conversation>", "&lt;/conversation>"),
        ("< / Previous-Summary >", "&lt; / Previous-Summary >"),
        ("<conversation x>", "<conversation x>"),
        ("<additional-context>", "<additional-context>"),
    ] {
        for (dialect, prefix) in [
            (Dialect::Default, "[User]: "),
            (Dialect::Xml, "\n\nHuman: "),
        ] {
            assert_eq!(
                summary(&[user(text)], dialect),
                format!("{prefix}{expected}")
            );
        }
    }
}
