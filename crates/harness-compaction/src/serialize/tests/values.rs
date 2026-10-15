use crate::serialize::{
    json::{es_number, quote, Json},
    ordered_arguments,
    transcript::thinking_block,
};

#[test]
fn numbers_render_like_ecmascript_strings() {
    for (value, expected) in [
        (1.0, "1"),
        (-0.0, "0"),
        (100.0, "100"),
        (2.50, "2.5"),
        (-0.5, "-0.5"),
        (0.000001, "0.000001"),
        (1e-7, "1e-7"),
        (1.5e-7, "1.5e-7"),
        (123456789012345680000.0, "123456789012345680000"),
        (1e21, "1e+21"),
        (1.5e300, "1.5e+300"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        assert_eq!(es_number(value), expected, "{value:?}");
    }
}

#[test]
fn compact_json_follows_object_enumeration_and_stringify_escaping() {
    for (text, expected) in [
        (
            r#"{ "b": 1, "2": 2, "a": 3, "1": 4, "b": 5, "01": 6 }"#,
            r#"{"1":4,"2":2,"b":5,"a":3,"01":6}"#,
        ),
        (
            r#"[1.0, -0, 9007199254740993, 1e2]"#,
            "[1,0,9007199254740992,100]",
        ),
        (r#""a/<>&\u2028é\u0001""#, "\"a/<>&\u{2028}é\\u0001\""),
        (
            r#"{"k":[null,true,{"x":"y"}]}"#,
            r#"{"k":[null,true,{"x":"y"}]}"#,
        ),
    ] {
        let parsed = Json::parse(text);
        assert_eq!(parsed.map(|json| json.compact()).as_deref(), Some(expected));
    }
    assert!(Json::parse("{").is_none());
    assert_eq!(quote("\u{1f}\u{8}\u{c}\t"), r#""\u001f\b\f\t""#);
    assert_eq!(
        ordered_arguments(r#"{"z":"v","a":[1,2]}"#),
        Some(vec![
            ("z".into(), "\"v\"".into()),
            ("a".into(), "[1,2]".into())
        ])
    );
    assert_eq!(ordered_arguments("[1]"), None);
}

#[test]
fn thinking_envelopes_unwrap_only_when_they_cover_the_whole_text() {
    for (thinking, expected) in [
        ("", ""),
        ("plain", "<thinking>\nplain\n</thinking>"),
        (
            "<thinking>\nCheck logs first.\n</thinking>",
            "<thinking>\nCheck logs first.\n</thinking>",
        ),
        (
            "<thinking>\nfirst\n</thinking>\n<thinking>\nsecond\n</thinking>",
            "<thinking>\nfirst\nsecond\n</thinking>",
        ),
        ("  <thinking>a</thinking>\t", "<thinking>\na\n</thinking>"),
        (
            "<thinking><thinking>a</thinking></thinking>",
            "<thinking>\na\n</thinking>",
        ),
        (
            "<thinking>a</thinking> tail",
            "<thinking>\n<thinking>a</thinking> tail\n</thinking>",
        ),
        ("<thinking>a", "<thinking>\n<thinking>a\n</thinking>"),
        (
            "<think>a</think>",
            "<thinking>\n<think>a</think>\n</thinking>",
        ),
    ] {
        assert_eq!(
            thinking_block("<thinking>", "</thinking>", thinking),
            expected,
            "{thinking:?}"
        );
    }
}
