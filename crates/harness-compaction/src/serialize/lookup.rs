//! Model identifier to transcript dialect.
//!
//! Only the identifier is known. Matching follows a provider-less, lenient taxonomy: a
//! provider-agnostic override wins, then one effort suffix is collapsed, then the most
//! specific class matcher decides. Unknown and ambiguous identifiers use XML.
use super::{transcript::ecma_trim, Dialect};

/// Effort and mode suffixes that do not change the model family.
const SUFFIXES: [&str; 9] = [
    "-thinking",
    "-extra-high",
    "-none",
    "-minimal",
    "-medium",
    "-xhigh",
    "-high",
    "-low",
    "-max",
];

const EXACT_OVERRIDES: [(&str, Dialect); 4] = [
    ("daybreak-blue-latest", Dialect::Xml),
    ("daybreak-red-latest", Dialect::Xml),
    ("gpt-daybreak-blue-latest", Dialect::Harmony),
    ("gpt-daybreak-red-latest", Dialect::Harmony),
];

const GLOB_OVERRIDES: [(&str, Dialect); 2] = [
    ("*bonsai-27b*", Dialect::Qwen3),
    ("*bonsai-2-27b*", Dialect::Qwen3),
];

#[derive(Clone, Copy)]
enum Kind {
    Exact,
    Bounded,
    Namespace,
    BoundedNamespace,
    Prefix,
    Glob,
}

impl Kind {
    fn rank(self) -> u8 {
        match self {
            Kind::Exact => 4,
            Kind::Bounded => 3,
            Kind::Namespace | Kind::BoundedNamespace => 2,
            Kind::Prefix => 1,
            Kind::Glob => 0,
        }
    }
}

use Kind::{
    Bounded as B, BoundedNamespace as NB, Exact as E, Glob as G, Namespace as N, Prefix as P,
};

type Matchers = &'static [(Kind, &'static str)];

/// Every class, including the ones that fall back to XML, because they compete.
const CLASSES: [(&str, Dialect, Matchers); 21] = [
    ("ai21", Dialect::Xml, &[(B, "jamba"), (N, "ai21")]),
    (
        "amazon",
        Dialect::Xml,
        &[(B, "nova"), (B, "titan"), (NB, "amazon")],
    ),
    (
        "anthropic",
        Dialect::Anthropic,
        &[(NB, "anthropic"), (B, "anthropic"), (B, "claude")],
    ),
    ("baidu", Dialect::Xml, &[(B, "ernie")]),
    ("bytedance", Dialect::Xml, &[(B, "doubao")]),
    ("cohere", Dialect::Xml, &[(B, "command"), (N, "cohere")]),
    (
        "deepseek",
        Dialect::DeepSeek,
        &[(NB, "deepseek"), (B, "deepseek")],
    ),
    ("gemini", Dialect::Gemini, &[(B, "gemini")]),
    ("gemma", Dialect::Gemma, &[(B, "gemma")]),
    ("glm", Dialect::Glm, &[(B, "glm"), (B, "zai-glm")]),
    (
        "gpt-oss",
        Dialect::Harmony,
        &[(NB, "gpt-oss"), (B, "gpt-oss")],
    ),
    (
        "kimi",
        Dialect::Kimi,
        &[(N, "moonshotai"), (B, "kimi"), (E, "k3"), (E, "k3-256k")],
    ),
    (
        "meta",
        Dialect::Xml,
        &[
            (B, "muse-spark"),
            (B, "muse-wonder"),
            (B, "llama"),
            (N, "meta-llama"),
        ],
    ),
    ("mimo", Dialect::Xml, &[(B, "mimo")]),
    (
        "minimax",
        Dialect::Minimax,
        &[(N, "minimax"), (B, "minimax"), (B, "hailuo")],
    ),
    ("mistral", Dialect::Xml, &[(B, "mistral"), (B, "mixtral")]),
    (
        "openai",
        Dialect::Harmony,
        &[
            (N, "openai"),
            (NB, "openai"),
            (P, "gpt-"),
            (P, "chatgpt-"),
            (P, "codex-"),
            (E, "o1"),
            (P, "o1-"),
            (P, "o1."),
            (E, "o3"),
            (P, "o3-"),
            (P, "o3."),
            (E, "o4"),
            (P, "o4-"),
            (P, "o4."),
        ],
    ),
    (
        "qwen",
        Dialect::Qwen3,
        &[
            (B, "qwen"),
            (B, "deepseek-r1-distill-qwen"),
            (G, "*distill-qwen*"),
        ],
    ),
    ("stepfun", Dialect::Xml, &[(B, "step")]),
    (
        "xai",
        Dialect::Xml,
        &[(P, "cursor-grok-"), (B, "grok"), (N, "x-ai"), (NB, "xai")],
    ),
    ("unknown", Dialect::Xml, &[]),
];

/// Select the transcript dialect for a model identifier. Hermes is never selected here.
pub fn dialect_for_model(model_id: &str) -> Dialect {
    let trimmed = ecma_trim(model_id);
    let basename = trimmed.rsplit('/').next().unwrap_or(trimmed).to_lowercase();
    if let Some(dialect) = override_dialect(&basename) {
        return dialect;
    }
    let id = if trimmed.len() == model_id.len() {
        collapse_suffix(trimmed)
    } else {
        trimmed.to_lowercase()
    };
    classify(&id)
}

fn override_dialect(basename: &str) -> Option<Dialect> {
    if let Some((_, dialect)) = EXACT_OVERRIDES.iter().find(|(name, _)| *name == basename) {
        return Some(*dialect);
    }
    let specificity = |pattern: &str| pattern.chars().filter(|c| *c != '*').count();
    let mut best: Option<(usize, Dialect)> = None;
    let mut tied = false;
    for (pattern, dialect) in GLOB_OVERRIDES {
        if !glob_matches(pattern, basename) {
            continue;
        }
        let score = specificity(pattern);
        match best {
            Some((top, _)) if score == top => tied = true,
            Some((top, _)) if score < top => {}
            _ => {
                best = Some((score, dialect));
                tied = false;
            }
        }
    }
    best.filter(|_| !tied).map(|(_, dialect)| dialect)
}

/// Lowercase the identifier, removing at most the longest matching effort suffix.
fn collapse_suffix(id: &str) -> String {
    let lower = id.to_lowercase();
    let basename = lower.rsplit('/').next().unwrap_or(&lower);
    let suffix = SUFFIXES
        .iter()
        .filter(|suffix| lower.len() > suffix.len() && lower.ends_with(**suffix))
        .filter(|suffix| **suffix != "-max" || !basename.starts_with("qwen"))
        .max_by_key(|suffix| suffix.len());
    match suffix {
        Some(suffix) => lower[..lower.len() - suffix.len()].to_owned(),
        None => lower,
    }
}

fn classify(id: &str) -> Dialect {
    let bare = id.rsplit('/').next().unwrap_or(id);
    let mut best: Option<((u8, usize), &str, Dialect)> = None;
    let mut tied = false;
    for (class, dialect, matchers) in CLASSES {
        for (kind, token) in matchers {
            if !matches_token(*kind, token, id, bare) {
                continue;
            }
            let score = (kind.rank(), token.encode_utf16().count());
            match best {
                Some((top, name, _)) if score == top => tied |= name != class,
                Some((top, _, _)) if score < top => {}
                _ => {
                    best = Some((score, class, dialect));
                    tied = false;
                }
            }
        }
    }
    match best {
        Some((_, _, dialect)) if !tied => dialect,
        _ => Dialect::Xml,
    }
}

fn matches_token(kind: Kind, token: &str, id: &str, bare: &str) -> bool {
    match kind {
        Kind::Exact => bare == token,
        Kind::Bounded => bounded(token, bare),
        Kind::Namespace => id.split('/').any(|segment| segment == token),
        Kind::BoundedNamespace => id
            .split(['/', '.', ':'])
            .filter(|segment| !segment.is_empty())
            .any(|segment| bounded(token, segment)),
        Kind::Prefix => bare.starts_with(token),
        Kind::Glob => glob_matches(token, bare),
    }
}

/// `text` equals `token` or continues it with a separator or digit.
fn bounded(token: &str, text: &str) -> bool {
    text.strip_prefix(token).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|c| matches!(c, '-' | '_' | '.' | ':') || c.is_ascii_digit())
    })
}

/// Anchored match where `*` is any substring and everything else is literal.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = parts.collect();
    let Some(last) = parts.pop() else {
        return rest.is_empty();
    };
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}
