use super::AnthropicTier;

/// Model class evidence from the matcher vocabulary. Only the first eight and
/// `Gemini`/`Xai` map to a distinct `Family`; the rest collapse to `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Anthropic,
    DeepSeek,
    Glm,
    Kimi,
    OpenAi,
    Qwen,
    Ai21,
    Amazon,
    Baidu,
    Bytedance,
    Cohere,
    Gemini,
    Gemma,
    GptOss,
    Meta,
    Mimo,
    Minimax,
    Mistral,
    Stepfun,
    Xai,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Exact,
    Bounded,
    Namespace,
    NamespaceBounded,
    Prefix,
    Glob,
}

impl Kind {
    /// Exact > bounded > namespace > prefix > glob.
    const fn rank(self) -> u8 {
        match self {
            Self::Exact => 5,
            Self::Bounded => 4,
            Self::Namespace | Self::NamespaceBounded => 3,
            Self::Prefix => 2,
            Self::Glob => 1,
        }
    }
}

use Class as C;
use Kind::{
    Bounded as B, Exact as E, Glob as G, Namespace as N, NamespaceBounded as NB, Prefix as P,
};

const MATCHERS: &[(Class, Kind, &str)] = &[
    (C::Anthropic, NB, "anthropic"),
    (C::Anthropic, B, "anthropic"),
    (C::Anthropic, B, "claude"),
    (C::DeepSeek, NB, "deepseek"),
    (C::DeepSeek, B, "deepseek"),
    (C::Glm, B, "glm"),
    (C::Glm, B, "zai-glm"),
    (C::Kimi, N, "moonshotai"),
    (C::Kimi, B, "kimi"),
    (C::Kimi, E, "k3"),
    (C::Kimi, E, "k3-256k"),
    (C::OpenAi, N, "openai"),
    (C::OpenAi, NB, "openai"),
    (C::OpenAi, P, "gpt-"),
    (C::OpenAi, P, "chatgpt-"),
    (C::OpenAi, P, "codex-"),
    (C::OpenAi, P, "o1-"),
    (C::OpenAi, P, "o1."),
    (C::OpenAi, P, "o3-"),
    (C::OpenAi, P, "o3."),
    (C::OpenAi, P, "o4-"),
    (C::OpenAi, P, "o4."),
    (C::OpenAi, E, "o1"),
    (C::OpenAi, E, "o3"),
    (C::OpenAi, E, "o4"),
    (C::Qwen, B, "qwen"),
    (C::Qwen, B, "deepseek-r1-distill-qwen"),
    (C::Qwen, G, "*distill-qwen*"),
    (C::Ai21, B, "jamba"),
    (C::Ai21, N, "ai21"),
    (C::Amazon, B, "nova"),
    (C::Amazon, B, "titan"),
    (C::Amazon, NB, "amazon"),
    (C::Baidu, B, "ernie"),
    (C::Bytedance, B, "doubao"),
    (C::Cohere, B, "command"),
    (C::Cohere, N, "cohere"),
    (C::Gemini, B, "gemini"),
    (C::Gemma, B, "gemma"),
    (C::GptOss, B, "gpt-oss"),
    (C::GptOss, NB, "gpt-oss"),
    (C::Meta, B, "muse-spark"),
    (C::Meta, B, "muse-wonder"),
    (C::Meta, B, "llama"),
    (C::Meta, N, "meta-llama"),
    (C::Mimo, B, "mimo"),
    (C::Minimax, B, "minimax"),
    (C::Minimax, B, "hailuo"),
    (C::Minimax, N, "minimax"),
    (C::Mistral, B, "mistral"),
    (C::Mistral, B, "mixtral"),
    (C::Stepfun, B, "step"),
    (C::Xai, P, "cursor-grok-"),
    (C::Xai, B, "grok"),
    (C::Xai, N, "x-ai"),
    (C::Xai, NB, "xai"),
];

/// Best class for a lowercase, trimmed, collapsed id. Equal best matches from
/// different classes make the id unknown.
pub(super) fn best_class(id: &str) -> Option<Class> {
    let mut best: Option<((u8, usize), Class)> = None;
    let mut tied = false;
    for &(class, kind, token) in MATCHERS {
        if !matches_token(kind, token, id) {
            continue;
        }
        let score = (kind.rank(), literal_len(token));
        match best {
            Some((top, _)) if score < top => {}
            Some((top, top_class)) if score == top => tied |= top_class != class,
            _ => {
                best = Some((score, class));
                tied = false;
            }
        }
    }
    if tied {
        None
    } else {
        best.map(|(_, class)| class)
    }
}

fn matches_token(kind: Kind, token: &str, id: &str) -> bool {
    match kind {
        Kind::Exact => id == token,
        Kind::Bounded => bounded(id, token),
        Kind::Namespace => id.split('/').any(|part| part == token),
        Kind::NamespaceBounded => id.split(['/', '.', ':']).any(|part| bounded(part, token)),
        Kind::Prefix => id.starts_with(token),
        Kind::Glob => glob_match(token, id),
    }
}

/// The token itself, or the token followed by `-`, `_`, `.`, `:` or an ASCII digit.
fn bounded(id: &str, token: &str) -> bool {
    id.strip_prefix(token).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|next| matches!(next, '-' | '_' | '.' | ':') || next.is_ascii_digit())
    })
}

/// Number of non-`*` characters of a glob pattern.
pub(super) fn literal_len(pattern: &str) -> usize {
    pattern.chars().filter(|&c| c != '*').count()
}

/// `*` matches any sequence including the empty one; everything else is literal.
pub(super) fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        if pattern.get(p) == Some(&b'*') {
            backtrack = Some((p, t));
            p += 1;
        } else if pattern.get(p) == Some(&text[t]) {
            p += 1;
            t += 1;
        } else if let Some((star, matched)) = backtrack {
            p = star + 1;
            t = matched + 1;
            backtrack = Some((star, matched + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&b| b == b'*')
}

const TIER_GLOBS: [(&str, AnthropicTier); 5] = [
    ("opus", AnthropicTier::Opus),
    ("sonnet", AnthropicTier::Sonnet),
    ("haiku", AnthropicTier::Haiku),
    ("fable", AnthropicTier::Fable),
    ("mythos", AnthropicTier::Mythos),
];

enum TierMatch {
    Found(AnthropicTier),
    Tie,
    Missing,
}

/// Tier from the `*opus*`-style substring globs: the longer literal wins and
/// equal different tiers are unknown. A tie on `vendor.model` or `vendor:model`
/// retries once without the `anthropic` namespace.
pub(super) fn anthropic_tier(id: &str) -> AnthropicTier {
    match detect_tier(id) {
        TierMatch::Found(tier) => tier,
        TierMatch::Missing => AnthropicTier::Unknown,
        TierMatch::Tie => match strip_anthropic_namespace(id).map(detect_tier) {
            Some(TierMatch::Found(tier)) => tier,
            _ => AnthropicTier::Unknown,
        },
    }
}

fn detect_tier(id: &str) -> TierMatch {
    let mut best: Option<(usize, AnthropicTier)> = None;
    let mut tied = false;
    for (literal, tier) in TIER_GLOBS {
        if !id.contains(literal) {
            continue;
        }
        match best {
            Some((len, _)) if literal.len() < len => {}
            Some((len, top)) if literal.len() == len => tied |= top != tier,
            _ => {
                best = Some((literal.len(), tier));
                tied = false;
            }
        }
    }
    match best {
        None => TierMatch::Missing,
        Some(_) if tied => TierMatch::Tie,
        Some((_, tier)) => TierMatch::Found(tier),
    }
}

fn strip_anthropic_namespace(id: &str) -> Option<&str> {
    let mut start = 0;
    for (index, c) in id.char_indices() {
        if c == '.' || c == ':' {
            if id.get(start..index) == Some("anthropic") {
                return Some(&id[index + 1..]);
            }
            start = index + 1;
        }
    }
    None
}
