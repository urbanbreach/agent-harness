//! Model names select behavior independently of provider transport and capabilities.

pub(crate) fn resolve(model: &str, metadata_family: Option<&str>) -> &'static str {
    let names: Vec<_> = std::iter::once(model)
        .chain(metadata_family)
        .map(|name| name.to_ascii_lowercase().replace(['.', '_'], "-"))
        .collect();
    for name in &names {
        if let Some((_, preset)) = VERSIONS.iter().find(|(marker, _)| matches(name, marker)) {
            return preset;
        }
    }
    for name in names.iter().rev() {
        if let Some((_, preset)) = FAMILIES.iter().find(|(marker, _)| matches(name, marker)) {
            return preset;
        }
    }
    "default"
}

fn matches(name: &str, marker: &str) -> bool {
    name.match_indices(marker).any(|(at, matched)| {
        let before = name.as_bytes().get(at.wrapping_sub(1));
        let after = name.as_bytes().get(at + matched.len());
        before.is_none_or(|byte| !byte.is_ascii_alphanumeric())
            && after.is_none_or(|byte| !byte.is_ascii_alphanumeric())
    })
}

pub(super) fn eval_dialect(preset: &str) -> &'static str {
    if preset.starts_with("gpt") {
        "gpt"
    } else if preset.starts_with("claude") || preset.starts_with("glm") {
        "claude"
    } else if preset.starts_with("kimi") || preset == "swe-2" {
        "kimi"
    } else if matches!(preset, "codex" | "openai-reasoning") {
        "codex"
    } else {
        "default"
    }
}

const VERSIONS: &[(&str, &str)] = &[
    ("kimi-for-coding-highspeed", "kimi-k2.7"),
    ("deepseek-v4-flash-0731", "deepseek-v4-flash-0731"),
    ("claude-mythos-preview", "claude-fable-5"),
    ("deepseek-v4-1-flash", "deepseek-v4.1-flash"),
    ("claude-mythos-5-1", "claude-fable-5.1"),
    ("claude-sonnet-5-5", "claude-sonnet-5.5"),
    ("deepseek-v4-flash", "deepseek-v4-flash"),
    ("claude-fable-5-1", "claude-fable-5.1"),
    ("claude-mythos-5", "claude-fable-5"),
    ("claude-opus-4-5", "claude-opus-4.5"),
    ("claude-opus-4-6", "claude-opus-4.6"),
    ("claude-opus-4-7", "claude-opus-4.7"),
    ("claude-opus-4-8", "claude-opus-4.8"),
    ("claude-opus-5-5", "claude-opus-5.5"),
    ("deepseek-v4-pro", "deepseek-v4-pro"),
    ("kimi-for-coding", "kimi-k2.8"),
    ("claude-fable-5", "claude-fable-5"),
    ("claude-opus-5", "claude-opus-5"),
    ("gpt-5-3-codex", "gpt-5.3-codex"),
    ("gpt-5-6-terra", "gpt-5.6-terra"),
    ("gpt-5-6-luna", "gpt-5.6-luna"),
    ("gpt-5-6-sol", "gpt-5.6-sol"),
    ("gpt-6-1-sol", "gpt-6.1-sol"),
    ("gpt-6-astra", "gpt-6-astra"),
    ("gpt-6-luna", "gpt-6-luna"),
    ("gpt-6-sol", "gpt-6-sol"),
    ("gpt-astra", "gpt-6-astra"),
    ("kimi-k2-6", "kimi-k2.6"),
    ("kimi-k2-7", "kimi-k2.7"),
    ("kimi-k2-8", "kimi-k2.8"),
    ("grok-4-5", "grok-4.5"),
    ("grok-4-6", "grok-4.6"),
    ("grok-4-7", "grok-4.7"),
    ("glm-5-2", "glm-5.2"),
    ("glm-5-3", "glm-5.3"),
    ("gpt-5-2", "gpt-5.2"),
    ("gpt-5-4", "gpt-5.4"),
    ("gpt-5-5", "gpt-5.5"),
    ("gpt-5-6", "gpt-5.6"),
    ("kimi-k3", "kimi-k3"),
    ("swe-2", "swe-2"),
    ("k2p6", "kimi-k2.6"),
    ("k2p7", "kimi-k2.7"),
    ("k2p8", "kimi-k2.8"),
];

const FAMILIES: &[(&str, &str)] = &[
    ("gpt-6", "gpt-6"),
    ("gpt-5", "gpt-5"),
    ("gpt5", "gpt-5"),
    ("codex", "codex"),
    ("gpt", "gpt"),
    ("openai-reasoning", "openai-reasoning"),
    ("o1", "openai-reasoning"),
    ("o3", "openai-reasoning"),
    ("o4", "openai-reasoning"),
    ("claude", "claude"),
    ("glm", "glm"),
    ("glm5", "glm"),
    ("kimi", "kimi"),
    ("grok", "grok"),
    ("deepseek", "deepseek"),
    ("gemini", "gemini"),
    ("minimax", "minimax"),
    ("mistral", "mistral"),
    ("llama", "llama"),
];
