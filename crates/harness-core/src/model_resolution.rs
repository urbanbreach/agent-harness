// allow: SIZE_OK — model resolution (variant + capability inference)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    OpenAiReasoning,
    Gpt5,
    GptLegacy,
    Codex,
    ClaudeOpus,
    Claude,
    Gemini,
    KimiThinking,
    Kimi,
    Glm,
    MiniMax,
    DeepSeek,
    Grok,
    Mistral,
    Llama,
    Trinity,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamilySource {
    Metadata,
    Heuristic,
    DefaultFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptFamily {
    Reasoning,
    Codex,
    Gpt,
    Anthropic,
    Gemini,
    Kimi,
    Trinity,
    Default,
}

impl PromptFamily {
    pub fn id(self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::Codex => "codex",
            Self::Gpt => "gpt",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Kimi => "kimi",
            Self::Trinity => "trinity",
            Self::Default => "default",
        }
    }

    pub fn data_asset_file(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("anthropic.md"),
            Self::Gemini => Some("gemini.md"),
            Self::Kimi => Some("kimi.md"),
            Self::Trinity => Some("trinity.md"),
            _ => None,
        }
    }

    pub fn data_asset_families() -> &'static [Self] {
        &[Self::Anthropic, Self::Gemini, Self::Kimi, Self::Trinity]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub variants: Vec<String>,
    pub reasoning_efforts: Vec<String>,
    pub supports_tool_calls: bool,
    pub supports_vision: bool,
    pub supports_temperature: bool,
    pub supports_top_p: bool,
    pub supports_thinking: bool,
    pub supports_reasoning_summaries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResolution {
    pub family: ModelFamily,
    pub family_source: ModelFamilySource,
    pub prompt_family: PromptFamily,
    pub capabilities: ModelCapabilities,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelResolutionInput<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub metadata_family: Option<&'a str>,
    pub input_modalities: &'a [String],
    pub supports_tool_calls: Option<bool>,
    pub supports_reasoning_summaries: Option<bool>,
}

impl Default for ModelResolution {
    fn default() -> Self {
        resolve_model(ModelResolutionInput {
            provider: "",
            model: "",
            metadata_family: None,
            input_modalities: &[],
            supports_tool_calls: None,
            supports_reasoning_summaries: None,
        })
    }
}

pub fn resolve_model(input: ModelResolutionInput<'_>) -> ModelResolution {
    let (family, family_source) = input
        .metadata_family
        .and_then(detect_metadata_family)
        .map(|family| (family, ModelFamilySource::Metadata))
        .or_else(|| {
            detect_heuristic_family(input.provider, input.model)
                .map(|family| (family, ModelFamilySource::Heuristic))
        })
        .unwrap_or((ModelFamily::Unknown, ModelFamilySource::DefaultFallback));

    let mut caps = family_capabilities(family);
    caps.supports_tool_calls = input
        .supports_tool_calls
        .unwrap_or(caps.supports_tool_calls);
    caps.supports_reasoning_summaries = input
        .supports_reasoning_summaries
        .unwrap_or(caps.supports_reasoning_summaries);
    caps.supports_vision =
        input_modalities_support_vision(input.input_modalities) || caps.supports_vision;

    ModelResolution {
        family,
        family_source,
        prompt_family: prompt_family_for(family),
        capabilities: caps,
    }
}

fn detect_metadata_family(raw: &str) -> Option<ModelFamily> {
    let normalized = normalize_family_token(raw);
    detect_family_from_normalized(&normalized)
}

fn detect_heuristic_family(provider: &str, model: &str) -> Option<ModelFamily> {
    let normalized_provider = normalize_family_token(provider);
    let model_name = extract_model_name(model);
    let normalized_model = normalize_family_token(&model_name);
    let combined = normalize_family_token(&format!("{provider}/{model}"));

    if matches!(
        normalized_provider.as_str(),
        "google" | "google-vertex" | "github-copilot"
    ) && normalized_model.starts_with("gemini")
    {
        return Some(ModelFamily::Gemini);
    }

    detect_family_from_normalized(&normalized_model)
        .or_else(|| detect_family_from_normalized(&combined))
}

fn detect_family_from_normalized(value: &str) -> Option<ModelFamily> {
    if value.contains("codex") && value.contains("gpt") {
        return Some(ModelFamily::Codex);
    }
    if starts_with_openai_reasoning_model(value) {
        return Some(ModelFamily::OpenAiReasoning);
    }
    if value.contains("claude-opus") {
        return Some(ModelFamily::ClaudeOpus);
    }
    if value.contains("claude") {
        return Some(ModelFamily::Claude);
    }
    if value.starts_with("gemini") || value.contains("-gemini") || value.contains("gemini-") {
        return Some(ModelFamily::Gemini);
    }
    if value.contains("kimi-thinking")
        || value.contains("k2-thinking")
        || value.contains("k2-think")
    {
        return Some(ModelFamily::KimiThinking);
    }
    if value.contains("kimi") || value.contains("k2") {
        return Some(ModelFamily::Kimi);
    }
    if value.contains("minimax") {
        return Some(ModelFamily::MiniMax);
    }
    if value.contains("deepseek") {
        return Some(ModelFamily::DeepSeek);
    }
    if value.contains("mistral") || value.contains("codestral") {
        return Some(ModelFamily::Mistral);
    }
    if value.contains("llama") {
        return Some(ModelFamily::Llama);
    }
    if value.contains("grok") {
        return Some(ModelFamily::Grok);
    }
    if value.contains("glm") {
        return Some(ModelFamily::Glm);
    }
    if value.contains("trinity") {
        return Some(ModelFamily::Trinity);
    }
    if value.contains("gpt-5") {
        return Some(ModelFamily::Gpt5);
    }
    if value.contains("gpt") {
        return Some(ModelFamily::GptLegacy);
    }
    None
}

fn starts_with_openai_reasoning_model(value: &str) -> bool {
    value.split('/').next_back().is_some_and(
        |model| matches!(model.as_bytes(), [b'o', digit, ..] if digit.is_ascii_digit()),
    )
}

fn extract_model_name(model: &str) -> String {
    model
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .unwrap_or(model)
        .to_string()
}

fn normalize_family_token(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| match ch {
            '_' | '.' | ' ' => '-',
            other => other,
        })
        .collect()
}

fn prompt_family_for(family: ModelFamily) -> PromptFamily {
    match family {
        ModelFamily::OpenAiReasoning => PromptFamily::Reasoning,
        ModelFamily::Codex => PromptFamily::Codex,
        ModelFamily::Gpt5 | ModelFamily::GptLegacy => PromptFamily::Gpt,
        ModelFamily::ClaudeOpus | ModelFamily::Claude => PromptFamily::Anthropic,
        ModelFamily::Gemini => PromptFamily::Gemini,
        ModelFamily::KimiThinking | ModelFamily::Kimi => PromptFamily::Kimi,
        ModelFamily::Trinity => PromptFamily::Trinity,
        ModelFamily::Glm
        | ModelFamily::MiniMax
        | ModelFamily::DeepSeek
        | ModelFamily::Grok
        | ModelFamily::Mistral
        | ModelFamily::Llama
        | ModelFamily::Unknown => PromptFamily::Default,
    }
}

fn family_capabilities(family: ModelFamily) -> ModelCapabilities {
    let mut caps = ModelCapabilities {
        variants: variants(&[]),
        reasoning_efforts: variants(&[]),
        supports_tool_calls: true,
        supports_vision: false,
        supports_temperature: true,
        supports_top_p: true,
        supports_thinking: false,
        supports_reasoning_summaries: false,
    };

    match family {
        ModelFamily::ClaudeOpus => {
            caps.variants = variants(&["low", "medium", "high", "max"]);
            caps.supports_thinking = true;
        }
        ModelFamily::Claude => {
            caps.variants = variants(&["low", "medium", "high"]);
            caps.supports_thinking = true;
        }
        ModelFamily::OpenAiReasoning => {
            caps.variants = variants(&["low", "medium", "high"]);
            caps.reasoning_efforts = variants(&["none", "minimal", "low", "medium", "high"]);
            caps.supports_reasoning_summaries = true;
        }
        ModelFamily::Gpt5 | ModelFamily::Codex => {
            caps.variants = variants(&["low", "medium", "high", "xhigh"]);
            caps.reasoning_efforts =
                variants(&["none", "minimal", "low", "medium", "high", "xhigh", "max"]);
            caps.supports_reasoning_summaries = true;
        }
        ModelFamily::GptLegacy
        | ModelFamily::Gemini
        | ModelFamily::Glm
        | ModelFamily::MiniMax
        | ModelFamily::Mistral
        | ModelFamily::Llama => {
            caps.variants = variants(&["low", "medium", "high"]);
        }
        ModelFamily::Grok => {
            caps.variants = variants(&["low", "medium", "high"]);
            caps.reasoning_efforts = variants(&["low", "medium", "high"]);
        }
        ModelFamily::KimiThinking => {
            caps.variants = variants(&["low", "medium", "high"]);
            caps.supports_thinking = true;
        }
        ModelFamily::Kimi => {
            caps.variants = variants(&["low", "medium", "high"]);
            caps.supports_thinking = false;
        }
        ModelFamily::DeepSeek => {
            caps.variants = variants(&["low", "medium", "high"]);
            caps.reasoning_efforts = variants(&["high", "max"]);
        }
        ModelFamily::Trinity | ModelFamily::Unknown => {}
    }

    caps
}

fn variants(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn input_modalities_support_vision(input_modalities: &[String]) -> bool {
    input_modalities.iter().any(|modality| {
        matches!(
            modality.trim().to_ascii_lowercase().as_str(),
            "image" | "images" | "vision" | "pdf" | "video"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_family_beats_model_id_heuristic() {
        let resolution = resolve_model(ModelResolutionInput {
            provider: "github-copilot",
            model: "enterprise-alpha",
            metadata_family: Some("gemini-pro"),
            input_modalities: &[],
            supports_tool_calls: None,
            supports_reasoning_summaries: None,
        });

        assert_eq!(resolution.family, ModelFamily::Gemini);
        assert_eq!(resolution.family_source, ModelFamilySource::Metadata);
        assert_eq!(resolution.prompt_family, PromptFamily::Gemini);
    }

    #[test]
    fn capabilities_combine_family_defaults_and_metadata() {
        let input_modalities = vec!["text".to_string(), "image".to_string()];
        let resolution = resolve_model(ModelResolutionInput {
            provider: "openai",
            model: "gpt-5.5",
            metadata_family: None,
            input_modalities: &input_modalities,
            supports_tool_calls: Some(false),
            supports_reasoning_summaries: Some(false),
        });

        assert_eq!(resolution.family, ModelFamily::Gpt5);
        assert_eq!(
            resolution.capabilities.reasoning_efforts,
            variants(&["none", "minimal", "low", "medium", "high", "xhigh", "max"])
        );
        assert!(resolution.capabilities.supports_vision);
        assert!(!resolution.capabilities.supports_tool_calls);
        assert!(!resolution.capabilities.supports_reasoning_summaries);
    }

    #[test]
    fn unknown_models_use_explicit_default_fallback() {
        let resolution = resolve_model(ModelResolutionInput {
            provider: "local",
            model: "mystery-model",
            metadata_family: None,
            input_modalities: &[],
            supports_tool_calls: None,
            supports_reasoning_summaries: None,
        });

        assert_eq!(resolution.family, ModelFamily::Unknown);
        assert_eq!(resolution.family_source, ModelFamilySource::DefaultFallback);
        assert_eq!(resolution.prompt_family, PromptFamily::Default);
    }
}
