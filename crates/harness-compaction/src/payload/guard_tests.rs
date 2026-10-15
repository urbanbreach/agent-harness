use super::*;

fn rejection<'a>(message: &'a str, status: Option<u16>) -> Rejection<'a> {
    Rejection {
        message,
        status,
        payload_flag: false,
        overflow_flag: false,
        usage: None,
    }
}

#[test]
fn status_413_sets_payload_until_final_text_has_token_evidence() {
    for (status, message, flag, payload) in [
        (Some(413), "provider returned HTTP 413", false, true),
        (Some(413), "HTTP 413 (no body)", false, true),
        (
            Some(413),
            "413 Request Entity Too Large: request body exceeds limit",
            false,
            true,
        ),
        (
            Some(413),
            "HTTP 413: prompt is too long: 210000 tokens > 200000 maximum",
            false,
            false,
        ),
        (
            Some(413),
            "request_too_large: prompt has too many tokens",
            false,
            false,
        ),
        (Some(413), "prompt is too long: 210000 tokens", true, true),
        (Some(400), "prompt is too long", false, false),
        (Some(400), "payload too large", false, true),
        (None, "provider stream connection failed", false, false),
    ] {
        let facts = Rejection {
            payload_flag: flag,
            ..rejection(message, status)
        };
        assert_eq!(facts.is_payload(), payload, "{status:?} {message}");
    }
}

#[test]
fn overflow_comes_from_the_flag_or_text() {
    let flagged = Rejection {
        overflow_flag: true,
        ..rejection("provider returned HTTP 400", Some(400))
    };
    assert!(flagged.is_overflow());
    assert!(rejection("HTTP 413 (no body)", Some(413)).is_overflow());
    assert!(!rejection("payload too large", Some(413)).is_overflow());
}

#[test]
fn usage_overflow_prefers_context_tokens_and_is_strict() {
    for (usage, window, overflow) in [
        (
            ReportedUsage {
                raw_input: 150,
                context_tokens: Some(210),
            },
            200,
            true,
        ),
        (
            ReportedUsage {
                raw_input: 250,
                context_tokens: Some(150),
            },
            200,
            false,
        ),
        (
            ReportedUsage {
                raw_input: 201,
                context_tokens: None,
            },
            200,
            true,
        ),
        (
            ReportedUsage {
                raw_input: 200,
                context_tokens: None,
            },
            200,
            false,
        ),
    ] {
        let facts = Rejection {
            usage: Some(usage),
            ..rejection("HTTP 413", Some(413))
        };
        assert_eq!(
            Evidence::assess(&facts, Some(window), 10).usage_overflow,
            overflow,
            "{usage:?}"
        );
    }
    let unknown = Rejection {
        usage: Some(ReportedUsage {
            raw_input: 900,
            context_tokens: None,
        }),
        ..rejection("HTTP 413", Some(413))
    };
    assert!(!Evidence::assess(&unknown, None, 10).usage_overflow);
}

#[test]
fn trusted_needs_raw_input_in_window_and_local_under_ninety_percent() {
    for (raw, local, window, trusted) in [
        (0, 899, Some(1000), true),
        (0, 900, Some(1000), false),
        (1000, 100, Some(1000), true),
        (1001, 100, Some(1000), false),
        (0, 100, None, false),
    ] {
        let facts = Rejection {
            usage: (raw > 0).then_some(ReportedUsage {
                raw_input: raw,
                context_tokens: Some(0),
            }),
            ..rejection("HTTP 413", Some(413))
        };
        assert_eq!(
            Evidence::assess(&facts, window, local).trusted,
            trusted,
            "{raw} {local} {window:?}"
        );
    }
}

#[test]
fn media_evidence_needs_a_limit_signal() {
    for (message, media) in [
        ("unsupported content type image/png", false),
        ("payload too large: image attached", false),
        (
            "payload too large: too many images (limit of 20 images)",
            true,
        ),
        ("image dimensions exceed 8000 pixels", true),
    ] {
        let facts = rejection(message, Some(413));
        assert_eq!(
            Evidence::assess(&facts, Some(1000), 10).media,
            media,
            "{message}"
        );
    }
}

#[test]
fn archive_is_excluded_for_payload_unless_usage_proves_overflow_without_media() {
    let usage = Some(ReportedUsage {
        raw_input: 2000,
        context_tokens: None,
    });
    for (message, usage, window, excluded) in [
        ("HTTP 413 (no body)", None, Some(1000), true),
        ("HTTP 413 (no body)", usage, Some(1000), false),
        (
            "payload too large: too many images",
            usage,
            Some(1000),
            true,
        ),
        ("HTTP 413 (no body)", usage, None, true),
        ("connection reset", None, Some(1000), false),
    ] {
        let facts = Rejection {
            usage,
            ..rejection(message, Some(413))
        };
        let facts = if message == "connection reset" {
            Rejection {
                status: None,
                ..facts
            }
        } else {
            facts
        };
        assert_eq!(
            Evidence::assess(&facts, window, 100).exclude_archive(),
            excluded,
            "{message} {window:?}"
        );
    }
}

const USABLE: Capabilities = Capabilities {
    usable: true,
    reclaimable: true,
};

#[test]
fn verdicts_follow_the_decision_sequence() {
    let bytes = |headroom| Verdict::Terminal(DeadEnd::Bytes { headroom });
    let shrink = |exclude_media, promote| Verdict::Shrink {
        exclude_media,
        promote,
    };
    let big = Some(ReportedUsage {
        raw_input: 5000,
        context_tokens: None,
    });
    for (name, facts, window, local, caps, expected) in [
        (
            "not payload",
            rejection("prompt is too long", Some(400)),
            Some(1000),
            100,
            USABLE,
            Verdict::NotPayload,
        ),
        (
            "trusted with reclaimable history shrinks without promotion or archive",
            rejection("HTTP 413 (no body)", Some(413)),
            Some(1000),
            100,
            USABLE,
            shrink(true, false),
        ),
        (
            "trusted without reclaimable history is terminal with stored headroom",
            rejection("HTTP 413 (no body)", Some(413)),
            Some(1000),
            100,
            Capabilities {
                usable: true,
                reclaimable: false,
            },
            bytes(Some(900)),
        ),
        (
            "unknown window and no usable method is terminal",
            rejection("HTTP 413 (no body)", Some(413)),
            None,
            100,
            Capabilities {
                usable: false,
                reclaimable: false,
            },
            bytes(None),
        ),
        (
            "unknown window with a usable method still shrinks and may promote",
            rejection("HTTP 413 (no body)", Some(413)),
            None,
            100,
            USABLE,
            shrink(true, true),
        ),
        (
            "untrusted known window shrinks and may promote",
            rejection("HTTP 413 (no body)", Some(413)),
            Some(1000),
            950,
            USABLE,
            shrink(true, true),
        ),
        (
            "media evidence without usage proof is terminal even near the window",
            rejection("payload too large: too many images", Some(413)),
            Some(1000),
            950,
            USABLE,
            bytes(Some(50)),
        ),
        (
            "media evidence with usage overflow may still shrink without archive",
            Rejection {
                usage: big,
                ..rejection("payload too large: too many images", Some(413))
            },
            Some(1000),
            100,
            USABLE,
            shrink(true, true),
        ),
        (
            "usage overflow without usable compaction is token shaped",
            Rejection {
                usage: big,
                ..rejection("HTTP 413 (no body)", Some(413))
            },
            Some(1000),
            100,
            Capabilities {
                usable: false,
                reclaimable: false,
            },
            Verdict::Terminal(DeadEnd::Tokens {
                reported: 5000,
                window: Some(1000),
            }),
        ),
        (
            "usage overflow keeps archive available",
            Rejection {
                usage: big,
                ..rejection("HTTP 413 (no body)", Some(413))
            },
            Some(1000),
            100,
            USABLE,
            shrink(false, true),
        ),
    ] {
        assert_eq!(
            Evidence::assess(&facts, window, local).verdict(caps),
            expected,
            "{name}"
        );
    }
}

#[test]
fn stored_headroom_never_goes_negative() {
    let facts = rejection("HTTP 413 (no body)", Some(413));
    let verdict = Evidence::assess(&facts, Some(1000), 1500).verdict(Capabilities {
        usable: false,
        reclaimable: false,
    });
    assert_eq!(
        verdict,
        Verdict::Terminal(DeadEnd::Bytes { headroom: Some(0) })
    );
}

#[test]
fn dead_end_warnings_are_byte_or_token_shaped() {
    assert!(DeadEnd::Bytes {
        headroom: Some(900)
    }
    .warning()
    .contains("~900 tokens of headroom"));
    assert!(DeadEnd::Bytes { headroom: None }
        .warning()
        .contains("no known context window"));
    assert!(DeadEnd::Tokens {
        reported: 5000,
        window: Some(1000)
    }
    .warning()
    .contains("~5,000 input tokens against a 1,000-token"));
}

#[test]
fn body_read_timeouts_are_told_apart_from_other_failures() {
    for (status, text, expected) in [
        (Some(408), "provider returned HTTP 408", true),
        (None, "request body read timeout", true),
        (None, "Timed out reading request body", true),
        (None, "provider stream connection failed", false),
        (Some(504), "provider returned HTTP 504", false),
        (Some(413), "HTTP 413 (no body)", false),
    ] {
        assert_eq!(is_body_read_timeout(status, text), expected, "{text}");
    }
}
