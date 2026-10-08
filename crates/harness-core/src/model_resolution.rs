use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    OpenAiReasoning,
    Gpt5,
    GptAstra,
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
    Mistral,
    Llama,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamilySource {
    Metadata,
    Heuristic,
    DefaultFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelResolution {
    pub family: ModelFamily,
    pub family_source: ModelFamilySource,
    /// Editable Markdown preset selected for this model, independent of transport capabilities.
    #[serde(default)]
    pub prompt_preset: String,
    pub capabilities: ModelCapabilities,
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

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct ModelResolutionInput<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub metadata_family: Option<&'a str>,
    pub input_modalities: &'a [String],
    pub supports_tool_calls: Option<bool>,
    pub supports_reasoning_summaries: Option<bool>,
}

pub fn resolve_model(input: ModelResolutionInput<'_>) -> ModelResolution {
    use ModelFamily as Family;
    let name = input
        .metadata_family
        .unwrap_or(input.model)
        .to_ascii_lowercase()
        .replace('_', "-");
    let family = [
        ("gpt-astra", Family::GptAstra),
        ("gpt-6", Family::GptAstra),
        ("codex", Family::Codex),
        ("gpt-5", Family::Gpt5),
        ("gpt5", Family::Gpt5),
        ("claude-opus", Family::ClaudeOpus),
        ("claude", Family::Claude),
        ("gemini", Family::Gemini),
        ("kimi-thinking", Family::KimiThinking),
        ("kimi", Family::Kimi),
        ("glm", Family::Glm),
        ("minimax", Family::MiniMax),
        ("deepseek", Family::DeepSeek),
        ("mistral", Family::Mistral),
        ("llama", Family::Llama),
        ("gpt", Family::GptLegacy),
    ]
    .into_iter()
    .find_map(|(prefix, family)| name.contains(prefix).then_some(family))
    .unwrap_or_else(|| {
        if ["o1", "o3", "o4", "openai-reasoning"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        {
            Family::OpenAiReasoning
        } else {
            Family::Unknown
        }
    });
    let reasoning = matches!(
        family,
        Family::OpenAiReasoning | Family::GptAstra | Family::Gpt5 | Family::Codex
    );
    ModelResolution {
        family,
        prompt_preset: crate::system_prompt::models::resolve(input.model, input.metadata_family)
            .into(),
        family_source: if input.metadata_family.is_some() {
            ModelFamilySource::Metadata
        } else if family == Family::Unknown {
            ModelFamilySource::DefaultFallback
        } else {
            ModelFamilySource::Heuristic
        },
        capabilities: ModelCapabilities {
            variants: Vec::new(),
            reasoning_efforts: if reasoning {
                ["low", "medium", "high"].map(str::to_owned).to_vec()
            } else {
                Vec::new()
            },
            supports_tool_calls: input
                .supports_tool_calls
                .unwrap_or(family != Family::Unknown),
            supports_vision: input.input_modalities.iter().any(|m| m == "image"),
            supports_temperature: !reasoning,
            supports_top_p: !reasoning,
            supports_thinking: matches!(
                family,
                Family::ClaudeOpus | Family::Claude | Family::Gemini | Family::KimiThinking
            ),
            supports_reasoning_summaries: input.supports_reasoning_summaries.unwrap_or(reasoning),
        },
    }
}
