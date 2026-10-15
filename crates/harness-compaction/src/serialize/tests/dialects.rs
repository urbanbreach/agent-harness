use super::{assistant, developer, native, result, summary, useless, user, Turn, ALL};
use crate::serialize::{
    chatml, legacy,
    transcript::{Assistant, Entry},
    Dialect,
};
use Dialect::*;

const BOS: &str = "<｜begin▁of▁sentence｜>";

/// user `u`, developer `d`, assistant `a`, result `r` of tool `read`.
fn basic() -> [Turn; 4] {
    [
        user("u"),
        developer("d"),
        assistant("a", &[], &[]),
        result("x", "read", "r"),
    ]
}

#[test]
fn complete_transcripts_match_the_native_formats() {
    let function_results = "<function_results>\n<result>\n<tool_name>read</tool_name>\n<stdout>r</stdout>\n</result>\n</function_results>";
    let chat = |tool_role: &str| {
        format!("<|im_start|>user\nu<|im_end|>\n<|im_start|>system\nd<|im_end|>\n<|im_start|>assistant\na<|im_end|>\n<|im_start|>{tool_role}\n<tool_response>\nr\n</tool_response><|im_end|>\n")
    };
    for (dialect, expected) in [
        (Default, "[User]: u\n\n[Assistant]: a\n\n[Tool Result]: r".to_owned()),
        (Xml, "\n\nHuman: u\n\nd\n\nAssistant: a\n\nHuman: <tool_response>\nr\n</tool_response>".to_owned()),
        (Minimax, format!("\n\nHuman: u\n\nd\n\nAssistant: a\n\nHuman: {function_results}")),
        (Anthropic, format!("\n\nHuman: u\n\nd\n\nAssistant: a\n\nHuman: {function_results}")),
        (Harmony, "<|start|>user<|message|>u<|end|><|start|>developer<|message|>d<|end|><|start|>assistant<|channel|>final<|message|>a<|end|><|start|>functions.read to=assistant<|channel|>commentary<|message|>r<|end|>".to_owned()),
        (Hermes, chat("tool")),
        (Qwen3, chat("user")),
        (Gemini, "<bos><start_of_turn>user\nu<end_of_turn>\n<start_of_turn>user\nd<end_of_turn>\n<start_of_turn>model\na```tool_code\n[]\n```<end_of_turn>\n<start_of_turn>user\n```tool_outputs\nr\n```<end_of_turn>\n".to_owned()),
        (Gemma, "<bos><|turn>user\nu<turn|><|turn>system\nd<turn|><|turn>model\na<|tool_response>response:read{output:<|\"|>r<|\"|>}<tool_response|><turn|>".to_owned()),
        (Glm, "[gMASK]<sop><|user|>\nu<|system|>\nd<|assistant|>\na<|observation|>\n<tool_response>\nr\n</tool_response>".to_owned()),
        (DeepSeek, format!("{BOS}<｜User｜>ud<｜Assistant｜>a<｜end▁of▁sentence｜><｜tool▁output▁begin｜>r<｜tool▁output▁end｜>")),
        (Kimi, "<|im_user|>user<|im_middle|>u<|im_end|><|im_system|>system<|im_middle|>d<|im_end|><|im_assistant|>assistant<|im_middle|>a<|im_end|><|im_system|>read<|im_middle|>## Return of functions.read:0\nr<|im_end|>".to_owned()),
    ] {
        assert_eq!(native(&basic(), dialect), expected, "{dialect:?}");
        if dialect != Harmony {
            assert_eq!(summary(&basic(), dialect), expected, "{dialect:?}");
        }
    }
    assert_eq!(
        summary(&basic(), Harmony),
        "<\\|start\\|>user<\\|message\\|>u<\\|end\\|><\\|start\\|>developer<\\|message\\|>d<\\|end\\|><\\|start\\|>assistant<\\|channel\\|>final<\\|message\\|>a<\\|end\\|><\\|start\\|>functions.read to=assistant<\\|channel\\|>commentary<\\|message\\|>r<\\|end\\|>"
    );
}

#[test]
fn a_lone_result_run_renders_each_dialect_result_form() {
    let file = [result("functions.read:0", "read", "FILE")];
    for (dialect, expected) in [
        (Glm, "[gMASK]<sop><|observation|>\n<tool_response>\nFILE\n</tool_response>"),
        (DeepSeek, "<｜begin▁of▁sentence｜><｜tool▁output▁begin｜>FILE<｜tool▁output▁end｜>"),
        (Kimi, "<|im_system|>read<|im_middle|>## Return of functions.read:0\nFILE<|im_end|>"),
        (Harmony, "<|start|>functions.read to=assistant<|channel|>commentary<|message|>FILE<|end|>"),
        (Xml, "\n\nHuman: <tool_response>\nFILE\n</tool_response>"),
        (Minimax, "\n\nHuman: <function_results>\n<result>\n<tool_name>read</tool_name>\n<stdout>FILE</stdout>\n</result>\n</function_results>"),
        (Qwen3, "<|im_start|>user\n<tool_response>\nFILE\n</tool_response><|im_end|>\n"),
        (Gemini, "<bos><start_of_turn>user\n```tool_outputs\nFILE\n```<end_of_turn>\n"),
        (Gemma, "<bos><|turn>model\n<|tool_response>response:read{output:<|\"|>FILE<|\"|>}<tool_response|><turn|>"),
    ] {
        assert_eq!(native(&file, dialect), expected, "{dialect:?}");
    }
    let views = [result("a", "read<", "boom")];
    let mut items = super::items(&views);
    items[0].tool.is_error = true;
    assert_eq!(
        crate::serialize::render(&items, Anthropic),
        "\n\nHuman: <function_results>\n<error>\n<tool_name>read&lt;</tool_name>\n<stderr>boom</stderr>\n</error>\n</function_results>"
    );
}

#[test]
fn thinking_blocks_use_each_dialects_fence() {
    let turns = [assistant("", &["reasoning"], &[])];
    for (dialect, expected) in [
        (Gemma, "<bos><|turn>model\n<|channel>thought\nreasoning<channel|><turn|>"),
        (Gemini, "<bos><start_of_turn>model\n```thinking\nreasoning\n```"),
        (Kimi, "<|im_assistant|>assistant<|im_middle|><think>\nreasoning\n</think><|im_end|>"),
        (Glm, "[gMASK]<sop><|assistant|>\n\n<think>\nreasoning\n</think>"),
        (DeepSeek, "<｜begin▁of▁sentence｜><｜Assistant｜><think>\nreasoning\n</think><｜end▁of▁sentence｜>"),
        (Qwen3, "<|im_start|>assistant\n<think>\nreasoning\n</think><|im_end|>\n"),
        (Hermes, "<|im_start|>assistant\n<think>\nreasoning\n</think><|im_end|>\n"),
        (Xml, "\n\nAssistant: <thinking>\nreasoning\n</thinking>"),
        (Minimax, "\n\nAssistant: <thinking>\nreasoning\n</thinking>"),
        (Harmony, "<|start|>assistant<|channel|>analysis<|message|>reasoning<|end|>"),
    ] {
        let rendered = native(&turns, dialect);
        assert!(rendered.starts_with(expected), "{dialect:?}: {rendered:?}");
    }
    // Fences that wrap existing envelopes differ between the families.
    let wrapped = [assistant(
        "",
        &["<thinking>\nCheck logs first.\n</thinking>"],
        &[],
    )];
    assert_eq!(
        native(&wrapped, Hermes),
        "<|im_start|>assistant\n<think>\n<thinking>\nCheck logs first.\n</thinking>\n</think><|im_end|>\n"
    );
    assert_eq!(
        native(&wrapped, Qwen3),
        "<|im_start|>assistant\n<think>\n<thinking>\nCheck logs first.\n</thinking>\n</think><|im_end|>\n"
    );
    assert_eq!(
        native(&wrapped, Xml),
        "\n\nAssistant: <thinking>\nCheck logs first.\n</thinking>"
    );
}

#[test]
fn direct_renderers_keep_anthropic_thinking_and_envelopes() {
    let thinking = |text: &str| {
        [Entry::Assistant(Assistant {
            thinking: Some(text.into()),
            text: String::new(),
            has_text: false,
            calls: Vec::new(),
        })]
    };
    for (text, expected) in [
        (
            "<thinking>\nCheck logs first.\n</thinking>",
            "\n\nAssistant: <thinking>\nCheck logs first.\n</thinking>",
        ),
        (
            "<thinking>\nfirst\n</thinking>\n<thinking>\nsecond\n</thinking>",
            "\n\nAssistant: <thinking>\nfirst\nsecond\n</thinking>",
        ),
    ] {
        assert_eq!(legacy::render(Anthropic, &thinking(text)), expected);
    }
    assert_eq!(
        chatml::render(Qwen3, &thinking("<thinking>\nCheck logs first.\n</thinking>")),
        "<|im_start|>assistant\n<think>\n<thinking>\nCheck logs first.\n</thinking>\n</think><|im_end|>\n"
    );
}

#[test]
fn search_transcript_matches_the_native_markers() {
    let turns = [
        user("Find foo"),
        assistant(
            "Searching.",
            &["I should search."],
            &[("call-1", "search", r#"{"query":"foo"}"#)],
        ),
        result("call-1", "search", "result"),
        assistant("Done.", &[], &[]),
    ];
    for (dialect, expected) in [
        (Harmony, vec![
            "<|start|>user<|message|>Find foo<|end|>",
            "<|start|>assistant<|channel|>analysis<|message|>I should search.<|end|>",
            "<|start|>assistant<|channel|>final<|message|>Searching.<|end|>",
            "<|start|>assistant<|channel|>commentary to=functions.search<|message|>{\"query\":\"foo\"}<|call|>",
            "<|start|>functions.search to=assistant<|channel|>commentary<|message|>result<|end|>",
        ]),
        (Qwen3, vec![
            "<|im_start|>assistant\n<think>\nI should search.\n</think>",
            "<|im_start|>user\n<tool_response>\nresult\n</tool_response><|im_end|>\n",
        ]),
        (Glm, vec![
            "<|assistant|>\n\n<think>\nI should search.\n</think>",
            "<|observation|>\n<tool_response>\nresult\n</tool_response>",
        ]),
        (Xml, vec!["\n\nHuman: Find foo", "\n\nAssistant: <thinking>\nI should search.\n</thinking>"]),
        (Anthropic, vec!["\n\nHuman: Find foo", "<function_calls>", "<function_results>"]),
    ] {
        let rendered = native(&turns, dialect);
        for part in expected {
            assert!(rendered.contains(part), "{dialect:?}: {part:?} in {rendered:?}");
        }
    }
    assert!(!summary(&turns, Anthropic).contains("I should search."));
    let harmony = summary(&turns, Harmony);
    assert!(harmony.contains("<\\|channel\\|>analysis<\\|message\\|>I should search."));
    assert!(!harmony.contains("<|"));
}

const ARGS: &str = r#"{"q":"a\"b","n":2.50,"ok":true,"z":null,"l":[1,"x"]}"#;
const COMPACT: &str = r#"{"q":"a\"b","n":2.5,"ok":true,"z":null,"l":[1,"x"]}"#;

#[test]
fn tool_calls_render_arguments_in_each_dialect_syntax() {
    let turns = [assistant("", &[], &[("call-1", "search", ARGS)])];
    let invoke = r#"<invoke name="search"><parameter name="q">"a\"b"</parameter><parameter name="n">2.5</parameter><parameter name="ok">true</parameter><parameter name="z">null</parameter><parameter name="l">[1,"x"]</parameter></invoke>"#;
    for (dialect, expected) in [
        (Default, r#"[Tool Call]: search(q="a\"b", n=2.5, ok=true, z=null, l=[1,"x"])"#.to_owned()),
        (Xml, format!("\n\nAssistant: {invoke}")),
        (Minimax, format!("\n\nAssistant: <minimax:tool_call>\n{invoke}\n</minimax:tool_call>")),
        (Anthropic, format!("\n\nAssistant: <function_calls>\n{invoke}\n</function_calls>")),
        (Harmony, format!("<|start|>assistant<|channel|>commentary to=functions.search<|message|>{COMPACT}<|call|>")),
        (Hermes, format!("<|im_start|>assistant\n<tool_call>\n{{\"name\":\"search\",\"arguments\":{COMPACT}}}\n</tool_call><|im_end|>\n")),
        (Qwen3, format!("<|im_start|>assistant\n<tool_call>\n{{\"name\":\"search\",\"arguments\":{COMPACT}}}\n</tool_call><|im_end|>\n")),
        (Gemini, "<bos><start_of_turn>model\n```tool_code\ndefault_api.search(q=\"a\\\"b\", n=2.5, ok=True, z=None, l=[1, \"x\"])\n```<end_of_turn>\n".to_owned()),
        (Gemma, "<bos><|turn>model\n<|tool_call>call:search{q:<|\"|>a\"b<|\"|>,n:2.5,ok:true,z:null,l:[1,<|\"|>x<|\"|>]}<tool_call|><turn|>".to_owned()),
        (Glm, String::from("[gMASK]<sop><|assistant|>\n<tool_call>search\n<arg_key>q</arg_key>\n<arg_value>\"a\\\"b\"</arg_value>\n<arg_key>n</arg_key>\n<arg_value>2.5</arg_value>\n<arg_key>ok</arg_key>\n<arg_value>true</arg_value>\n<arg_key>z</arg_key>\n<arg_value>null</arg_value>\n<arg_key>l</arg_key>\n<arg_value>[1,\"x\"]</arg_value>\n</tool_call>")),
        (DeepSeek, format!("{BOS}<｜Assistant｜><｜tool▁calls▁begin｜><｜tool▁call▁begin｜>search<｜tool▁sep｜>{COMPACT}<｜tool▁call▁end｜><｜tool▁calls▁end｜><｜end▁of▁sentence｜>")),
        (Kimi, format!("<|im_assistant|>assistant<|im_middle|><|tool_calls_section_begin|><|tool_call_begin|>functions.search:0<|tool_call_argument_begin|>{COMPACT}<|tool_call_end|><|tool_calls_section_end|><|im_end|>")),
    ] {
        assert_eq!(native(&turns, dialect), expected, "{dialect:?}");
    }
}

#[test]
fn multiple_calls_join_per_dialect_rules() {
    let calls = [
        ("functions.first:7", "a&\"<>'", r#"{"k&\"":1}"#),
        ("other", "functions.b", "{}"),
    ];
    let turns = [
        assistant("t", &[], &calls),
        result("functions.first:7", "functions.b", "one"),
        result("other", "b", "two"),
    ];
    for (dialect, expected) in [
        (Xml, "\n\nAssistant: t<invoke name=\"a&amp;&quot;&lt;&gt;'\"><parameter name=\"k&amp;&quot;\">1</parameter></invoke>\n<invoke name=\"functions.b\"></invoke>\n\nHuman: <tool_response>\none\n</tool_response>\n<tool_response>\ntwo\n</tool_response>"),
        (Harmony, "<|start|>assistant<|channel|>final<|message|>t<|end|><|start|>assistant<|channel|>commentary to=functions.a&\"<>'<|message|>{\"k&\\\"\":1}<|call|><|start|>assistant<|channel|>commentary to=functions.b<|message|>{}<|call|><|start|>functions.b to=assistant<|channel|>commentary<|message|>one<|end|><|start|>functions.b to=assistant<|channel|>commentary<|message|>two<|end|>"),
        (Gemini, "<bos><start_of_turn>model\nt```tool_code\n[default_api.a&\"<>'(k&\"=1), default_api.functions.b()]\n```<end_of_turn>\n<start_of_turn>user\n```tool_outputs\none\n```\n```tool_outputs\ntwo\n```<end_of_turn>\n"),
        (Gemma, "<bos><|turn>model\nt<|tool_call>call:a&\"<>'{k&\":1}<tool_call|><|tool_call>call:functions.b{}<tool_call|><|tool_response>response:functions.b{output:<|\"|>one<|\"|>}<tool_response|><|tool_response>response:b{output:<|\"|>two<|\"|>}<tool_response|><turn|>"),
        (DeepSeek, "<｜begin▁of▁sentence｜><｜Assistant｜>t<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>a&\"<>'<｜tool▁sep｜>{\"k&\\\"\":1}<｜tool▁call▁end｜><｜tool▁call▁begin｜>functions.b<｜tool▁sep｜>{}<｜tool▁call▁end｜><｜tool▁calls▁end｜><｜end▁of▁sentence｜><｜tool▁output▁begin｜>one<｜tool▁output▁end｜>\n<｜tool▁output▁begin｜>two<｜tool▁output▁end｜>"),
        (Kimi, "<|im_assistant|>assistant<|im_middle|>t<|tool_calls_section_begin|><|tool_call_begin|>functions.first:7<|tool_call_argument_begin|>{\"k&\\\"\":1}<|tool_call_end|><|tool_call_begin|>functions.functions.b:1<|tool_call_argument_begin|>{}<|tool_call_end|><|tool_calls_section_end|><|im_end|><|im_system|>functions.b<|im_middle|>## Return of functions.first:7\none<|im_end|><|im_system|>b<|im_middle|>## Return of functions.b:1\ntwo<|im_end|>"),
    ] {
        assert_eq!(native(&turns, dialect), expected, "{dialect:?}");
    }
}

#[test]
fn gemini_merges_developer_text_into_the_next_user_turn() {
    for (turns, expected) in [
        (vec![developer("")], "<bos>"),
        (
            vec![developer("d")],
            "<bos><start_of_turn>user\nd<end_of_turn>\n",
        ),
        (
            vec![developer("d1"), developer(""), developer("d2"), user("u")],
            "<bos><start_of_turn>user\nd1\nd2\nu<end_of_turn>\n",
        ),
        (
            vec![developer("d"), user("")],
            "<bos><start_of_turn>user\nd<end_of_turn>\n",
        ),
        (vec![user("")], "<bos><start_of_turn>user\n<end_of_turn>\n"),
        (vec![], ""),
    ] {
        assert_eq!(native(&turns, Gemini), expected);
    }
}

#[test]
fn legacy_records_keep_empty_roles_and_developer_spacing() {
    for (turns, expected) in [
        (vec![developer("d"), user("u")], "d\n\nHuman: u"),
        (vec![developer(""), user("u")], "\n\nHuman: u"),
        (vec![user("u"), developer("d")], "\n\nHuman: u\n\nd"),
        (vec![user("")], "\n\nHuman: "),
        (vec![assistant("", &[""], &[])], "\n\nAssistant: "),
    ] {
        assert_eq!(native(&turns, Xml), expected);
    }
}

#[test]
fn empty_assistants_follow_each_dialect() {
    let blank = [assistant("", &[""], &[])];
    for (dialect, expected) in [
        (
            Harmony,
            "<|start|>assistant<|channel|>final<|message|><|end|>",
        ),
        (Hermes, "<|im_start|>assistant\n<|im_end|>\n"),
        (Qwen3, "<|im_start|>assistant\n<|im_end|>\n"),
        (
            Gemini,
            "<bos><start_of_turn>model\n```tool_code\n[]\n```<end_of_turn>\n",
        ),
        (Gemma, "<bos><|turn>model\n<turn|>"),
        (Glm, "[gMASK]<sop><|assistant|>\n"),
        (
            DeepSeek,
            "<｜begin▁of▁sentence｜><｜Assistant｜><｜end▁of▁sentence｜>",
        ),
        (Kimi, "<|im_assistant|>assistant<|im_middle|><|im_end|>"),
    ] {
        assert_eq!(native(&blank, dialect), expected, "{dialect:?}");
    }
    // An assistant reduced to nothing by useless-pair removal disappears entirely.
    let pair = [
        assistant("", &[], &[("u", "grep", "{}")]),
        useless(result("u", "grep", "no matches")),
    ];
    for dialect in ALL {
        assert_eq!(native(&pair, dialect), "", "{dialect:?}");
    }
}

#[test]
fn dropped_messages_make_results_adjacent() {
    let turns = [
        result("a", "read", "one"),
        assistant("", &[], &[("u", "grep", "{}")]),
        useless(result("u", "grep", "none")),
        result("b", "read", ""),
        result("c", "read", "two"),
    ];
    assert_eq!(
        native(&turns, Xml),
        "\n\nHuman: <tool_response>\none\n</tool_response>\n<tool_response>\ntwo\n</tool_response>"
    );
    assert_eq!(
        native(&turns, Kimi),
        "<|im_system|>read<|im_middle|>## Return of functions.read:0\none<|im_end|><|im_system|>read<|im_middle|>## Return of functions.read:1\ntwo<|im_end|>"
    );
}

#[test]
fn gemma_results_decode_json_output() {
    let turns = [
        assistant("", &[], &[]),
        result("a", "r", r#"{"a":[1,"b"],"c":null}"#),
        result("b", "r", "null"),
        result("c", "r", "42"),
        result("d", "r", "not json"),
    ];
    assert_eq!(
        native(&turns, Gemma),
        "<bos><|turn>model\n<|tool_response>response:r{output:{a:[1,<|\"|>b<|\"|>],c:null}}<tool_response|><|tool_response>response:r{output:null}<tool_response|><|tool_response>response:r{output:42}<tool_response|><|tool_response>response:r{output:<|\"|>not json<|\"|>}<tool_response|><turn|>"
    );
}

#[test]
fn harmony_neutralizes_every_reserved_marker_before_boundary_escaping() {
    let markers = "<|start|><|end|><|message|><|channel|><|constrain|><|return|><|call|><|Start|>";
    let turns = [
        user(&format!("{markers}</conversation>")),
        assistant("", &[], &[("a", "t", r#"{"x":"<|call|>"}"#)]),
    ];
    let rendered = summary(&turns, Harmony);
    assert!(rendered.contains(
        "<\\|start\\|><\\|end\\|><\\|message\\|><\\|channel\\|><\\|constrain\\|><\\|return\\|><\\|call\\|><|Start|>&lt;/conversation>"
    ));
    assert!(rendered.contains("{\"x\":\"<\\|call\\|>\"}<\\|call\\|>"));
    assert!(native(&turns, Harmony).contains("<|message|>{\"x\":\"<|call|>\"}<|call|>"));
}

#[test]
fn summary_transcripts_drop_images_from_results_and_mark_user_images() {
    let views = [user("see"), result("a", "read", "")];
    let mut items = super::items(&views);
    items[0].image_count = 2;
    items[1].image_count = 1;
    assert_eq!(
        crate::serialize::serialize_for_summary(&items, Xml),
        "\n\nHuman: see[Image][Image]"
    );
    assert_eq!(
        crate::serialize::serialize_for_summary(&items, Default),
        "[User]: see"
    );
}
