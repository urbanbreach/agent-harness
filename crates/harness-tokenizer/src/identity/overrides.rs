use super::classes::{glob_match, literal_len};
use super::revision::Revision;
use super::{AnthropicTier, Family, ModelIdentity};

/// Reviewed alias override. An empty provider list means any provider; patterns
/// are lowercase basenames (exact) or `*` globs.
struct Entry {
    providers: &'static [&'static str],
    pattern: &'static str,
    identity: ModelIdentity,
}

const fn entry(
    providers: &'static [&'static str],
    pattern: &'static str,
    family: Family,
    revision: Option<Revision>,
) -> Entry {
    Entry {
        providers,
        pattern,
        identity: ModelIdentity { family, revision },
    }
}

const OPUS: Family = Family::Anthropic {
    tier: AnthropicTier::Opus,
};
const ANY: &[&str] = &[];

const EXACT: &[Entry] = &[
    entry(
        &["nanogpt"],
        "gemma-4-31b-claude-4.6-opus-reasoning-distilled",
        OPUS,
        Some((4, 6, 0)),
    ),
    entry(
        &["nanogpt"],
        "qwen3.5-27b-claude-4.6-opus-reasoning-distilled-derestricted",
        OPUS,
        Some((4, 6, 0)),
    ),
    entry(
        &["nanogpt"],
        "qwen3.5-27b-claude-4.6-opus-reasoning-distilled-derestricted-lite",
        OPUS,
        Some((4, 6, 0)),
    ),
    entry(
        &["deepseek", concat!("open", "code-go")],
        "deepseek-flash",
        Family::DeepSeek,
        Some((4, 1, 0)),
    ),
    entry(
        &["vercel-ai-gateway"],
        "deepseek-v3.2-thinking",
        Family::DeepSeek,
        Some((3, 2, 0)),
    ),
    entry(
        &[concat!("open", "code-zen")],
        "big-pickle",
        Family::DeepSeek,
        None,
    ),
    entry(
        &["umans"],
        "umans-deepseek-v4-flash-0731",
        Family::DeepSeek,
        Some((4, 0, 0)),
    ),
    entry(
        &["umans"],
        "umans-deepseek-v4-flash-0731-lab",
        Family::DeepSeek,
        Some((4, 0, 0)),
    ),
    entry(
        &["venice"],
        "e2ee-deepseek-v4-flash",
        Family::DeepSeek,
        Some((4, 0, 0)),
    ),
    entry(
        &["umans"],
        "umans-glm-5.3-flash-lab",
        Family::Glm,
        Some((5, 3, 0)),
    ),
    entry(
        &["abliteration"],
        "abliterated-model-large-v2",
        Family::Glm,
        Some((5, 3, 0)),
    ),
    entry(
        &["abliteration"],
        "abliterated-model-large",
        Family::Glm,
        Some((5, 2, 0)),
    ),
    entry(&["abliteration"], "abliterated-model", Family::Glm, None),
    entry(
        &["kilo", "openrouter", "vercel-ai-gateway"],
        "kimi-k2-thinking",
        Family::Kimi,
        None,
    ),
    entry(ANY, "daybreak-blue-latest", Family::Other, Some((5, 6, 0))),
    entry(ANY, "daybreak-red-latest", Family::Other, Some((5, 6, 0))),
    entry(
        ANY,
        "gpt-daybreak-blue-latest",
        Family::OpenAi,
        Some((5, 6, 0)),
    ),
    entry(
        ANY,
        "gpt-daybreak-red-latest",
        Family::OpenAi,
        Some((5, 6, 0)),
    ),
    entry(&["yolo-auto"], "yolo", Family::Qwen, Some((3, 8, 0))),
    entry(&["kilo", "openrouter"], "qwq-32b", Family::Qwen, None),
    entry(&["nanogpt"], "eva-qwen2.5-32b-v0.2", Family::Qwen, None),
    entry(&["nanogpt"], "eva-qwen2.5-72b-v0.2", Family::Qwen, None),
    entry(&["nanogpt"], "qwenlong-l1-32b", Family::Qwen, None),
    entry(&["nanogpt"], "dolphin-2.9.2-qwen2-72b", Family::Qwen, None),
    entry(
        &["nanogpt"],
        "cogito-v1-preview-qwen-32b",
        Family::Qwen,
        None,
    ),
    entry(&["nanogpt"], "qwq-32b-preview", Family::Qwen, None),
    entry(&["nanogpt"], "grayline-qwen3-8b", Family::Qwen, None),
    entry(&["novita"], "deepseek-r1-0528-qwen3-8b", Family::Qwen, None),
    entry(&["umans"], "umans-qwen3.6-35b-a3b", Family::Qwen, None),
    entry(&["venice"], "e2ee-qwen-2-5-7b-p", Family::Qwen, None),
    entry(&["venice"], "e2ee-qwen3-30b-a3b-p", Family::Qwen, None),
    entry(&["venice"], "e2ee-qwen3-5-122b-a10b", Family::Qwen, None),
    entry(&["venice"], "e2ee-qwen3-6-27b", Family::Qwen, None),
    entry(&["venice"], "e2ee-qwen3-6-35b-a3b", Family::Qwen, None),
    entry(
        &["venice"],
        "e2ee-qwen3-6-35b-a3b-uncensored-p",
        Family::Qwen,
        None,
    ),
    entry(&["venice"], "e2ee-qwen3-vl-30b-a3b-p", Family::Qwen, None),
    entry(&["venice"], "e2ee-gpt-oss-120b-p", Family::Other, None),
    entry(&["venice"], "e2ee-gpt-oss-20b-p", Family::Other, None),
    entry(&["venice"], "openai-gpt-oss-120b", Family::Other, None),
    entry(&["venice"], "xiaomi-mimo-v2-5", Family::Other, None),
];

const GLOBS: &[Entry] = &[
    entry(ANY, "*bonsai-27b*", Family::Qwen, Some((3, 6, 0))),
    entry(ANY, "*bonsai-2-27b*", Family::Qwen, Some((3, 8, 0))),
];

/// Provider-specific exact, agnostic exact, provider-specific glob, then
/// agnostic glob. Equal best globs within a tier are ignored.
pub(super) fn find(provider: &str, id: &str) -> Option<ModelIdentity> {
    let for_provider = |entry: &Entry| entry.providers.contains(&provider);
    let agnostic = |entry: &Entry| entry.providers.is_empty();
    exact(id, for_provider)
        .or_else(|| exact(id, agnostic))
        .or_else(|| best_glob(id, for_provider))
        .or_else(|| best_glob(id, agnostic))
}

fn exact(id: &str, scope: impl Fn(&Entry) -> bool) -> Option<ModelIdentity> {
    EXACT
        .iter()
        .find(|entry| scope(entry) && entry.pattern == id)
        .map(|entry| entry.identity)
}

fn best_glob(id: &str, scope: impl Fn(&Entry) -> bool) -> Option<ModelIdentity> {
    let mut matches = GLOBS
        .iter()
        .filter(|entry| scope(entry) && glob_match(entry.pattern, id))
        .map(|entry| (literal_len(entry.pattern), entry.identity));
    let mut best = matches.next()?;
    let mut tied = false;
    for candidate in matches {
        if candidate.0 > best.0 {
            best = candidate;
            tied = false;
        } else if candidate.0 == best.0 {
            tied = true;
        }
    }
    (!tied).then_some(best.1)
}
