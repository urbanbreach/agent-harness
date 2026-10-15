use std::sync::LazyLock;

use crate::count::Encoding;

mod cache;
mod classes;
mod overrides;
mod revision;
#[cfg(test)]
mod tests;

use cache::BoundedCache;
use classes::Class;
use revision::Revision;

const RESOLUTION_CACHE_CAPACITY: usize = 2048;
const CLASSIFICATION_CACHE_CAPACITY: usize = 16384;

static RESOLUTIONS: LazyLock<BoundedCache<Option<Encoding>>> =
    LazyLock::new(|| BoundedCache::new(RESOLUTION_CACHE_CAPACITY));
static CLASSIFICATIONS: LazyLock<BoundedCache<ModelIdentity>> =
    LazyLock::new(|| BoundedCache::new(CLASSIFICATION_CACHE_CAPACITY));

/// Anthropic model tier from the `opus`/`sonnet`/`haiku`/`fable`/`mythos` globs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnthropicTier {
    Opus,
    Sonnet,
    Haiku,
    Fable,
    Mythos,
    Unknown,
}

/// Model family relevant to counting and image pricing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    OpenAi,
    Anthropic { tier: AnthropicTier },
    Gemini,
    Xai,
    Qwen,
    DeepSeek,
    Kimi,
    Glm,
    Other,
}

/// Classified model: family plus an optional `(major, minor, patch)` revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelIdentity {
    pub family: Family,
    pub revision: Option<(u32, u32, u32)>,
}

/// Leniently classify a model id for a provider.
pub fn classify(provider: &str, model_id: &str) -> ModelIdentity {
    CLASSIFICATIONS.get_or_insert_with(cache_key(provider, model_id), || {
        classify_uncached(provider, model_id)
    })
}

/// Native encoding for a provider and model id, or `None` when the family has no
/// shipped vocabulary for that revision.
pub fn encoding_for(provider: &str, model_id: &str) -> Option<Encoding> {
    RESOLUTIONS.get_or_insert_with(cache_key(provider, model_id), || {
        select_encoding(classify(provider, model_id))
    })
}

fn cache_key(provider: &str, model_id: &str) -> String {
    format!("{provider}\0{model_id}")
}

fn classify_uncached(provider: &str, model_id: &str) -> ModelIdentity {
    let provider = provider.trim().to_lowercase();
    let basename = model_id.rsplit('/').next().unwrap_or(model_id);
    let id = basename.trim().to_lowercase();
    if let Some(forced) = overrides::find(&provider, &id) {
        return forced;
    }
    let id = if basename.trim().len() == basename.len() {
        collapse_effort(&provider, id)
    } else {
        id
    };
    let Some(class) = classes::best_class(&id) else {
        return ModelIdentity {
            family: Family::Other,
            revision: None,
        };
    };
    let family = match class {
        Class::Anthropic => Family::Anthropic {
            tier: classes::anthropic_tier(&id),
        },
        Class::OpenAi => Family::OpenAi,
        Class::Gemini => Family::Gemini,
        Class::Xai => Family::Xai,
        Class::Qwen => Family::Qwen,
        Class::DeepSeek => Family::DeepSeek,
        Class::Kimi => Family::Kimi,
        Class::Glm => Family::Glm,
        _ => Family::Other,
    };
    ModelIdentity {
        family,
        revision: revision::parse(class, &id),
    }
}

/// Longest matching effort suffix, longest first. `-max` stays on `qwen` ids and
/// the Cursor provider collapses the effort before `-fast`.
const EFFORT_SUFFIXES: [&str; 9] = [
    "-extra-high",
    "-thinking",
    "-minimal",
    "-medium",
    "-xhigh",
    "-high",
    "-none",
    "-low",
    "-max",
];

fn collapse_effort(provider: &str, id: String) -> String {
    if provider == "cursor"
        && let Some(base) = id.strip_suffix("-fast")
    {
        return format!("{}-fast", collapse_suffix(base));
    }
    let collapsed = collapse_suffix(&id);
    if collapsed.len() == id.len() {
        id
    } else {
        collapsed.to_owned()
    }
}

fn collapse_suffix(id: &str) -> &str {
    for suffix in EFFORT_SUFFIXES {
        if suffix == "-max" && id.starts_with("qwen") {
            continue;
        }
        if let Some(base) = id.strip_suffix(suffix)
            && !base.is_empty()
        {
            return base;
        }
    }
    id
}

fn select_encoding(identity: ModelIdentity) -> Option<Encoding> {
    let at_least = |minimum: Revision| {
        identity
            .revision
            .is_some_and(|revision| revision >= minimum)
    };
    match identity.family {
        Family::Anthropic { tier } => Some(match tier {
            AnthropicTier::Opus if at_least((5, 0, 0)) => Encoding::ClaudeV5,
            AnthropicTier::Opus if at_least((4, 7, 0)) => Encoding::ClaudeV47,
            AnthropicTier::Sonnet | AnthropicTier::Fable | AnthropicTier::Mythos
                if at_least((5, 0, 0)) =>
            {
                Encoding::ClaudeV5Sonnet
            }
            AnthropicTier::Haiku if at_least((5, 5, 0)) => Encoding::ClaudeV5Sonnet,
            _ => Encoding::ClaudeV3,
        }),
        Family::Qwen if at_least((3, 5, 0)) => Some(Encoding::Qwen3),
        Family::DeepSeek => Some(Encoding::DeepSeekV3),
        Family::Kimi => Some(Encoding::KimiK2),
        Family::Glm if at_least((5, 0, 0)) => Some(Encoding::Glm5),
        Family::OpenAi if at_least((5, 0, 0)) => Some(Encoding::O200k),
        _ => None,
    }
}
