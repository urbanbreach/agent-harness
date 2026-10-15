use super::*;

fn anthropic(tier: AnthropicTier) -> Family {
    Family::Anthropic { tier }
}

#[test]
fn classifies_family_and_revision() {
    let cases = [
        (
            "openrouter",
            "qwen/qwen3-32b",
            Family::Qwen,
            Some((3, 0, 0)),
        ),
        ("openai", "gpt-4o", Family::OpenAi, None),
        ("openai", "gpt-5.5", Family::OpenAi, Some((5, 5, 0))),
        ("openai", "o3", Family::OpenAi, None),
        ("openai", "o3-high", Family::OpenAi, None),
        ("zai", "glm-52", Family::Glm, Some((52, 0, 0))),
        ("moonshot", "moonshotai/opaque", Family::Other, None),
        (
            "anthropic",
            "claude-opus-5",
            anthropic(AnthropicTier::Opus),
            Some((5, 0, 0)),
        ),
        (
            "anthropic",
            "claude-haiku-5.5",
            anthropic(AnthropicTier::Haiku),
            Some((5, 5, 0)),
        ),
        (
            "anthropic",
            "claude-sonnet-4-20250514",
            anthropic(AnthropicTier::Sonnet),
            Some((4, 0, 0)),
        ),
        (
            "bedrock",
            "anthropic.claude-opus-4-1-20250805-v1:0",
            anthropic(AnthropicTier::Opus),
            Some((4, 1, 0)),
        ),
        (
            "anthropic",
            "claude-haiku-fable-5",
            anthropic(AnthropicTier::Unknown),
            Some((5, 0, 0)),
        ),
        ("google", "gemini-3-pro", Family::Gemini, Some((3, 0, 0))),
        ("google", "gemma-2", Family::Other, None),
        ("xai", "grok-4", Family::Xai, Some((4, 0, 0))),
        ("deepseek", "deepseek-chat", Family::DeepSeek, None),
        ("ollama", "deepseek-r1-distill-qwen-7b", Family::Qwen, None),
        (
            "openrouter",
            "moonshotai/kimi-k2-thinking",
            Family::Kimi,
            None,
        ),
    ];
    for (provider, model, family, revision) in cases {
        assert_eq!(
            classify(provider, model),
            ModelIdentity { family, revision },
            "{model}"
        );
    }
}

#[test]
fn selects_encodings() {
    use Encoding::*;
    let cases = [
        ("anthropic", "claude-opus-4-7", Some(ClaudeV47)),
        ("anthropic", "claude-opus-5", Some(ClaudeV5)),
        ("anthropic", "claude-opus-5-thinking", Some(ClaudeV5)),
        ("anthropic", "claude-sonnet-5", Some(ClaudeV5Sonnet)),
        ("anthropic", "claude-haiku-5.5", Some(ClaudeV5Sonnet)),
        ("anthropic", "claude-haiku-5", Some(ClaudeV3)),
        ("anthropic", "claude-3-opus-20240229", Some(ClaudeV3)),
        ("anthropic", "claude", Some(ClaudeV3)),
        ("anthropic", "claude-haiku-fable-5", Some(ClaudeV3)),
        ("deepseek", "deepseek-chat", Some(DeepSeekV3)),
        ("openrouter", "moonshotai/kimi-k2-thinking", Some(KimiK2)),
        ("openai", "gpt-5.5", Some(O200k)),
        ("openai", "gpt-4o", None),
        ("google", "gemini-3-pro", None),
        ("openrouter", "qwen3.5-27b", Some(Qwen3)),
        ("openrouter", "qwen3-32b", None),
        ("zai", "glm-5", Some(Glm5)),
        ("zai", "glm-4.6", None),
        (
            "vercel-ai-gateway",
            "deepseek-v3.2-thinking",
            Some(DeepSeekV3),
        ),
        ("yolo-auto", "yolo", Some(Qwen3)),
        ("other", "yolo", None),
        ("any", "x/daybreak-blue-latest", None),
        ("any", "gpt-daybreak-blue-latest", Some(O200k)),
        ("openrouter", "qwq-32b", None),
        ("any", "acme-bonsai-2-27b", Some(Qwen3)),
        (
            "Nanogpt",
            "Gemma-4-31B-Claude-4.6-Opus-Reasoning-Distilled",
            Some(ClaudeV3),
        ),
    ];
    for (provider, model, expected) in cases {
        assert_eq!(
            encoding_for(provider, model),
            expected,
            "{provider} {model}"
        );
        // The memoized answer is stable.
        assert_eq!(
            encoding_for(provider, model),
            expected,
            "{provider} {model} cached"
        );
    }
}

#[test]
fn effort_collapse_rules() {
    assert_eq!(collapse_effort("openai", "o3-extra-high".into()), "o3");
    assert_eq!(collapse_effort("openai", "qwen3-max".into()), "qwen3-max");
    assert_eq!(collapse_effort("openai", "gpt-5-max".into()), "gpt-5");
    assert_eq!(
        collapse_effort("cursor", "gpt-5-high-fast".into()),
        "gpt-5-fast"
    );
    // Padding changes the length, so the suffix is not collapsed.
    assert_eq!(classify("openai", " o3-high ").revision, Some((3, 0, 0)));
}
