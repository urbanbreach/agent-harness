use crate::{Counter, ImageDetail, ImageSize, Reader, ReaderApi};

fn reader(api: ReaderApi, provider: &str, model: &str) -> Counter {
    Counter::byte_estimating(Some(Reader {
        api,
        provider: provider.into(),
        model_id: model.into(),
        official_endpoint: true,
        prefix_binding: false,
        filter_reasoning_history: false,
    }))
}

fn size(width: u32, height: u32) -> Option<ImageSize> {
    Some(ImageSize { width, height })
}

#[test]
fn ordinary_image_prices_follow_the_lineage_rules() {
    let none = Counter::byte_estimating(None);
    let claude = reader(ReaderApi::Anthropic, "anthropic", "claude-sonnet-4-7");
    let gemini = reader(ReaderApi::Other, "google", "gemini-3-pro");
    let gpt61 = reader(ReaderApi::OpenAiResponses, "openai", "gpt-6.1");
    let opus47 = reader(ReaderApi::Anthropic, "anthropic", "claude-opus-4-7");
    let legacy = reader(
        ReaderApi::Anthropic,
        "anthropic",
        "claude-sonnet-4-5-20250929",
    );
    let high = Some(ImageDetail::High);
    let original = Some(ImageDetail::Original);
    let cases: [(&Counter, Option<ImageSize>, Option<ImageDetail>, u32); 16] = [
        (&none, size(1024, 1024), high, 1229),
        (&none, size(2048, 2048), high, 3000),
        (&none, size(4096, 512), original, 2458),
        (&none, None, high, 3000),
        (&none, None, original, 12000),
        (&none, None, None, 12000),
        (&none, size(1000, 1000), None, 1229),
        (&claude, size(1000, 1000), None, 1296),
        (&gemini, size(1000, 1000), None, 1120),
        (&gpt61, size(4000, 4000), original, 18750),
        (&opus47, size(4000, 2000), None, 4232),
        (&opus47, None, None, 4761),
        (&legacy, size(1, 1), None, 1),
        (&legacy, size(4000, 2000), None, 1568),
        (&claude, size(1, 1), None, 1),
        (&none, size(1, 1), None, 2),
    ];
    for (counter, dimensions, detail, expected) in cases {
        assert_eq!(counter.image_tokens(dimensions, detail), expected);
    }
}

#[test]
fn frame_prices_use_the_ceiling_without_a_rule_or_readable_header() {
    let none = Counter::byte_estimating(None);
    let claude = reader(ReaderApi::Anthropic, "anthropic", "claude-opus-4-7");
    let gpt = reader(ReaderApi::OpenAiResponses, "openai", "gpt-5.5");
    let other = reader(ReaderApi::Other, "mystery", "unknown-model");
    assert_eq!(none.frame_tokens(800, 600), 5024);
    assert_eq!(other.frame_tokens(800, 600), 5024);
    assert_eq!(claude.frame_tokens(0, 600), 5024);
    assert_eq!(claude.frame_tokens(800, 600), 29 * 22);
    assert_eq!(gpt.frame_tokens(1024, 1024), 1229);
    // Verified rules plan with their own price; unverified rules never plan below the ceiling.
    assert_eq!(claude.frame_planning_tokens(800, 600), 29 * 22);
    assert_eq!(gpt.frame_planning_tokens(1024, 1024), 1229);
    let wire = reader(ReaderApi::OpenAiResponses, "custom", "gpt-4o");
    assert_eq!(wire.frame_tokens(1024, 1024), 1229);
    assert_eq!(wire.frame_planning_tokens(1024, 1024), 5024);
    assert_ne!(claude.frame_key(), gpt.frame_key());
}

#[test]
fn wire_fallback_applies_when_lineage_is_unknown() {
    let openrouter = reader(ReaderApi::Other, "openrouter", "vendor/unknown-model");
    let chat = reader(ReaderApi::OpenAiChat, "custom", "local-model");
    assert_eq!(openrouter.image_tokens(size(1000, 1000), None), 1296);
    assert_eq!(chat.image_tokens(size(1000, 1000), None), 1229);
}
