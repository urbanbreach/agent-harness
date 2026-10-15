use crate::serialize::{dialect_for_model, Dialect};

#[test]
fn model_identifiers_select_the_most_specific_dialect() {
    use Dialect::*;
    for (id, expected) in [
        // Spec scenarios.
        ("org/GPT-OSS-120B", Harmony),
        ("deepseek-r1-distill-qwen-32b", Qwen3),
        ("openai/claude-x", Anthropic),
        ("Hermes-3", Xml),
        ("qwq-32b", Xml),
        ("org/Bonsai-2-27B-Q4.gguf", Qwen3),
        ("k3-thinking", Kimi),
        (" k3-thinking ", Xml),
        // One representative per class.
        ("CLAUDE-sonnet", Anthropic),
        ("anthropic.claude-3", Anthropic),
        ("gemini-2.5-pro", Gemini),
        ("gemma-3-27b", Gemma),
        ("glm-4.6", Glm),
        ("zai-glm-4.6", Glm),
        ("deepseek-v3.1", DeepSeek),
        ("deepseek/deepseek-chat", DeepSeek),
        ("moonshotai/Kimi-K2", Kimi),
        ("k3-256k", Kimi),
        ("MiniMax-M2", Minimax),
        ("hailuo-02", Minimax),
        ("gpt-5", Harmony),
        ("openai/o3-mini", Harmony),
        ("o1", Harmony),
        ("qwen3-coder", Qwen3),
        ("llama-3.1-70b", Xml),
        ("mistral-large", Xml),
        ("x-ai/grok-4", Xml),
        ("command-r", Xml),
        ("nova-pro", Xml),
        ("ernie-4.5", Xml),
        ("doubao-1.6", Xml),
        ("mimo-7b", Xml),
        ("step-3", Xml),
        ("jamba-1.5", Xml),
        ("", Xml),
        // Suffix collapse removes at most one suffix, never `-max` from qwen.
        ("o3-mini-high", Harmony),
        ("gemini-3-pro-extra-high", Gemini),
        ("qwen3-max", Qwen3),
        ("claude-opus-high-low", Anthropic),
        ("kimi-k2-thinking", Kimi),
        ("k3-low", Kimi),
        ("k3-low-low", Xml),
        // Overrides skip class matching.
        ("daybreak-blue-latest", Xml),
        ("gpt-daybreak-red-latest", Harmony),
        ("ORG/BONSAI-27B", Qwen3),
        // Equal best scores from different classes are ambiguous; a better score clears it.
        ("minimax/gpt-oss/foo", Xml),
        ("minimax/gpt-oss/gpt-oss-x", Harmony),
    ] {
        assert_eq!(dialect_for_model(id), expected, "{id:?}");
    }
}

#[test]
fn lookup_never_selects_the_default_or_hermes_dialect() {
    for id in [
        "hermes",
        "nous/hermes-3-llama-3.1",
        "unknown",
        "custom/model",
    ] {
        assert_eq!(dialect_for_model(id), Dialect::Xml, "{id:?}");
    }
}
