//! Parity fixtures for the native content counters.

use super::count;
use crate::Encoding;

const WHITESPACE: &str = "  \t\t\n\n\r\n   ";

fn check(encoding: Encoding, rows: &[(&str, u32)]) {
    for &(text, expected) in rows {
        assert_eq!(
            count(encoding, text),
            Some(expected),
            "{encoding:?} on {text:?}"
        );
    }
}

#[test]
fn bpe_families_match_native_fixtures() {
    for encoding in [
        Encoding::O200k,
        Encoding::Qwen3,
        Encoding::KimiK2,
        Encoding::Glm5,
    ] {
        check(
            encoding,
            &[("", 0), (" ", 1), ("hello world", 2), (WHITESPACE, 4)],
        );
    }
    check(
        Encoding::DeepSeekV3,
        &[("", 0), (" ", 1), ("hello world", 2), (WHITESPACE, 6)],
    );
}

#[test]
fn claude_families_match_native_fixtures() {
    // (text, v3, v4.7, opus 5) content counts, without the fixed message frame.
    let rows: &[(&str, u32, u32, u32)] = &[
        ("", 1, 1, 0),
        (" ", 1, 1, 0),
        ("hello, world", 3, 4, 4),
        ("Hello, World!", 4, 6, 6),
        ("NASA likes tokenizers", 4, 10, 10),
        ("The quick brown fox jumps over the lazy dog.", 11, 18, 18),
        ("it's", 2, 2, 2),
        ("a\0b", 2, 2, 2),
    ];
    for &(text, v3, v47, opus5) in rows {
        check(Encoding::ClaudeV3, &[(text, v3)]);
        check(Encoding::ClaudeV47, &[(text, v47)]);
        check(Encoding::ClaudeV5, &[(text, opus5)]);
    }
}

#[test]
fn claude_marker_lookalikes_are_priced_as_bytes() {
    let literal = "a\u{FDD0}b";
    let private_use = "\u{E000}\u{E001}\u{E002}";
    for encoding in [
        Encoding::ClaudeV3,
        Encoding::ClaudeV47,
        Encoding::ClaudeV5,
        Encoding::ClaudeV5Sonnet,
    ] {
        check(encoding, &[(literal, 5)]);
    }
    check(Encoding::ClaudeV3, &[(private_use, 1)]);
    check(Encoding::ClaudeV47, &[(private_use, 1)]);
    check(Encoding::ClaudeV5, &[(private_use, 0)]);
    check(Encoding::ClaudeV5Sonnet, &[(private_use, 0)]);
}

#[test]
fn claude_sonnet5_keeps_the_trailing_newline_ladder() {
    check(
        Encoding::ClaudeV5Sonnet,
        &[
            ("a ", 2),
            ("a\t", 2),
            ("a \n", 2),
            ("a\n ", 3),
            ("a \t \n ", 6),
            (" a ", 3),
            ("  'REXX  ", 6),
            ("日本語 text  ", 6),
        ],
    );
    // Opus 5 instead absorbs every trailing ASCII whitespace character.
    check(Encoding::ClaudeV5, &[("a ", 1), ("a \t \n ", 1)]);
}
