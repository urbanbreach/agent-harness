//! Image pricing rules: one table keyed by reader lineage, then wire api.
//!
//! Three regimes exist: OpenAI-style 32 px patches with per-detail edge and patch limits,
//! Anthropic-style 28 px patches fitted by padded edge, and a fixed price per image.

#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "Price formulas are defined over f64 pixel and patch arithmetic and clamp back to u32"
)]

use crate::count::{Reader, ReaderApi};
use crate::identity::{classify, AnthropicTier, Family, ModelIdentity};
use crate::image::{ImageDetail, ImageSize};

/// Price of an archive frame whose rule or dimensions are unknown.
pub(crate) const FRAME_CEILING_TOKENS: u32 = 5024;
/// Dimension assumed for an image whose size is unknown.
const UNKNOWN_EDGE: u32 = 65535;

/// Edge and patch limits for one OpenAI detail level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Level {
    pub max_edge: u32,
    pub patch_budget: Option<u32>,
}

const fn level(max_edge: u32, patch_budget: Option<u32>) -> Level {
    Level {
        max_edge,
        patch_budget,
    }
}

/// One image pricing regime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Regime {
    OpenAi {
        multiplier: f64,
        auto: ImageDetail,
        low: Level,
        high: Level,
        original: Level,
    },
    Anthropic {
        max_edge: u32,
        max_tokens: u32,
    },
    Fixed(u32),
}

/// A resolved regime and whether the reference verified it against billing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rule {
    pub regime: Regime,
    pub verified: bool,
}

const GPT_5_6: Regime = Regime::OpenAi {
    multiplier: 1.2,
    auto: ImageDetail::Original,
    low: level(512, None),
    high: level(2048, Some(2500)),
    original: level(65535, None),
};
const GPT_5_5: Regime = Regime::OpenAi {
    multiplier: 1.2,
    auto: ImageDetail::Original,
    low: level(512, None),
    high: level(2048, Some(2500)),
    original: level(6000, Some(10000)),
};
const GPT_5_4: Regime = Regime::OpenAi {
    multiplier: 1.2,
    auto: ImageDetail::High,
    low: level(2048, Some(6144)),
    high: level(2048, Some(2500)),
    original: level(6000, Some(10000)),
};
const GPT_5_2: Regime = Regime::OpenAi {
    multiplier: 1.2,
    auto: ImageDetail::High,
    low: level(2048, Some(6144)),
    high: level(2048, Some(6144)),
    original: level(2048, Some(6144)),
};
const GPT_ASTRA: Regime = Regime::OpenAi {
    multiplier: 1.2,
    auto: ImageDetail::Original,
    low: level(512, None),
    high: level(65535, Some(2500)),
    original: level(65535, None),
};
const GROK: Regime = Regime::OpenAi {
    multiplier: 1.017,
    auto: ImageDetail::Original,
    low: level(65535, Some(2401)),
    high: level(65535, Some(2401)),
    original: level(65535, Some(2401)),
};
const ANTHROPIC_DEFAULT: Regime = Regime::Anthropic {
    max_edge: 2576,
    max_tokens: 4784,
};
const ANTHROPIC_LEGACY: Regime = Regime::Anthropic {
    max_edge: 1568,
    max_tokens: 1568,
};

const fn rule(regime: Regime, verified: bool) -> Rule {
    Rule { regime, verified }
}

type Revision = (u32, u32, u32);

fn in_range(revision: Option<Revision>, from: Revision, below: Revision) -> bool {
    revision.is_some_and(|revision| revision >= from && revision < below)
}

/// Rule for the reader's model lineage, when the family has one.
fn lineage_rule(identity: ModelIdentity, model_id: &str) -> Option<Rule> {
    let revision = identity.revision;
    match identity.family {
        Family::OpenAi => {
            if model_id.to_lowercase().contains("gpt-6-astra") {
                Some(rule(GPT_ASTRA, true))
            } else if in_range(revision, (5, 6, 0), (10, 0, 0)) {
                Some(rule(GPT_5_6, true))
            } else if revision == Some((5, 5, 0)) {
                Some(rule(GPT_5_5, true))
            } else if revision == Some((5, 4, 0)) {
                Some(rule(GPT_5_4, false))
            } else if in_range(revision, (5, 2, 0), (5, 4, 0)) {
                Some(rule(GPT_5_2, false))
            } else {
                None
            }
        }
        Family::Anthropic { tier } => {
            let legacy = revision.is_some_and(|revision| {
                revision < (4, 7, 0) || ((10, 0, 0)..(47, 0, 0)).contains(&revision)
            });
            Some(match tier {
                AnthropicTier::Opus | AnthropicTier::Haiku if legacy => {
                    rule(ANTHROPIC_LEGACY, false)
                }
                AnthropicTier::Sonnet if legacy => {
                    let verified = in_range(revision, (4, 6, 0), (4, 7, 0))
                        || in_range(revision, (46, 0, 0), (47, 0, 0));
                    rule(ANTHROPIC_LEGACY, verified)
                }
                _ => rule(ANTHROPIC_DEFAULT, true),
            })
        }
        Family::Gemini if revision.is_some_and(|revision| revision >= (3, 0, 0)) => {
            Some(rule(Regime::Fixed(1120), false))
        }
        Family::Xai if revision.is_some_and(|r| ((4, 6, 0)..=(4, 20, 0)).contains(&r)) => {
            Some(rule(GROK, true))
        }
        _ => None,
    }
}

/// Providers whose wire api is not one of the three Harness protocols but whose images follow
/// the Anthropic or Google fallback.
fn provider_wire_rule(provider: &str) -> Option<Rule> {
    let provider = provider.trim().to_lowercase();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| provider.starts_with(prefix));
    if starts(&["google", "gemini", "vertex"]) {
        Some(rule(Regime::Fixed(1120), false))
    } else if starts(&[
        "openrouter",
        "ollama",
        "cursor",
        "factory",
        "gitlab",
        "devin",
        "apple",
        "bedrock",
    ]) {
        Some(rule(ANTHROPIC_DEFAULT, false))
    } else {
        None
    }
}

fn wire_rule(reader: &Reader) -> Option<Rule> {
    match reader.api {
        ReaderApi::OpenAiResponses | ReaderApi::OpenAiChat => Some(rule(GPT_5_5, false)),
        ReaderApi::Anthropic => Some(rule(ANTHROPIC_DEFAULT, false)),
        ReaderApi::Other => provider_wire_rule(&reader.provider),
    }
}

/// Lineage rule first, then the wire api rule. `None` when neither exists.
pub(crate) fn resolve(reader: Option<&Reader>) -> Option<Rule> {
    let reader = reader?;
    lineage_rule(
        classify(&reader.provider, &reader.model_id),
        &reader.model_id,
    )
    .or_else(|| wire_rule(reader))
}

/// The OpenAI Responses wire rule used when nothing else resolves.
pub(crate) fn fallback() -> Rule {
    rule(GPT_5_5, false)
}

fn ceil_div(value: u32, unit: u32) -> u64 {
    u64::from(value.div_ceil(unit))
}

fn level_for(regime: &Regime, detail: Option<ImageDetail>) -> Option<Level> {
    let Regime::OpenAi {
        auto,
        low,
        high,
        original,
        ..
    } = regime
    else {
        return None;
    };
    let detail = match detail {
        None | Some(ImageDetail::Auto) => *auto,
        Some(explicit) => explicit,
    };
    Some(match detail {
        ImageDetail::Low => *low,
        ImageDetail::High => *high,
        ImageDetail::Original | ImageDetail::Auto => *original,
    })
}

/// Price of one image of `size` under `regime`.
pub(crate) fn price(regime: &Regime, size: ImageSize, detail: Option<ImageDetail>) -> u32 {
    let width = size.width.max(1);
    let height = size.height.max(1);
    match *regime {
        Regime::Fixed(tokens) => tokens,
        Regime::Anthropic {
            max_edge,
            max_tokens,
        } => anthropic(width, height, max_edge, max_tokens),
        Regime::OpenAi { multiplier, .. } => {
            let level = level_for(regime, detail).unwrap_or(level(UNKNOWN_EDGE, None));
            let patches = openai_patches(width, height, level);
            saturating_u32((patches as f64 * multiplier).ceil())
        }
    }
}

fn saturating_u32(value: f64) -> u32 {
    if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value as u32
    }
}

fn openai_patches(width: u32, height: u32, level: Level) -> u64 {
    let (mut width, mut height) = (width, height);
    let longest = width.max(height);
    if longest > level.max_edge {
        let scale = f64::from(level.max_edge) / f64::from(longest);
        width = ((f64::from(width) * scale).round() as u32).max(1);
        height = ((f64::from(height) * scale).round() as u32).max(1);
    }
    let patches = ceil_div(width, 32) * ceil_div(height, 32);
    let Some(budget) = level.patch_budget.map(u64::from) else {
        return patches;
    };
    if patches <= budget {
        return patches;
    }
    let (w, h) = (f64::from(width), f64::from(height));
    let shrink = (1024.0 * budget as f64 / (w * h)).sqrt();
    let wide = w * shrink / 32.0;
    let tall = h * shrink / 32.0;
    let adjust = shrink * (wide.floor() / wide).min(tall.floor() / tall);
    let (new_w, new_h) = ((w * adjust).floor(), (h * adjust).floor());
    if new_w <= 0.0 || new_h <= 0.0 {
        return budget;
    }
    (ceil_div(new_w as u32, 32) * ceil_div(new_h as u32, 32)).min(budget)
}

fn anthropic_patches(width: u32, height: u32) -> u64 {
    ceil_div(width, 28) * ceil_div(height, 28)
}

fn anthropic_fits(width: u32, height: u32, max_edge: u32, max_tokens: u32) -> bool {
    let padded = |edge: u32| ceil_div(edge, 28) * 28;
    padded(width).max(padded(height)) <= u64::from(max_edge)
        && anthropic_patches(width, height) <= u64::from(max_tokens)
}

fn round_half_even(value: f64) -> f64 {
    let floor = value.floor();
    let diff = value - floor;
    if diff < 0.5 {
        floor
    } else if diff > 0.5 || floor % 2.0 != 0.0 {
        floor + 1.0
    } else {
        floor
    }
}

fn anthropic(width: u32, height: u32, max_edge: u32, max_tokens: u32) -> u32 {
    if anthropic_fits(width, height, max_edge, max_tokens) {
        return saturating_u32(anthropic_patches(width, height) as f64);
    }
    let (long, short) = (width.max(height), width.min(height));
    let aspect = f64::from(long) / f64::from(short);
    let sized = |edge: u32| {
        let other = (round_half_even(f64::from(edge) / aspect) as u32).max(1);
        if width >= height {
            (edge, other)
        } else {
            (other, edge)
        }
    };
    let (mut low, mut high) = (1u32, long);
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let (w, h) = sized(mid);
        if anthropic_fits(w, h, max_edge, max_tokens) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let (w, h) = sized(low);
    saturating_u32(anthropic_patches(w, h) as f64)
}

/// Stable text identity of a rule, used to compare billing domains.
pub(crate) fn key(rule: Option<Rule>, detail: Option<ImageDetail>) -> String {
    match rule {
        Some(rule) => format!(
            "{}:{:?}:{}",
            detail.map_or("auto".to_owned(), |detail| format!("{detail:?}")),
            rule.regime,
            rule.verified
        ),
        None => "ceiling".to_owned(),
    }
}

#[cfg(test)]
mod tests;
