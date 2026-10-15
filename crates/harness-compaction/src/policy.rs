//! Window-aware thresholds, preserving explicit reserve provenance.
use std::collections::BTreeMap;

mod editor;
pub use editor::{
    format_override, plan_override, preview, OverridePlan, OverrideWarning, PreviewOrigin,
    ThresholdPreview,
};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThresholdSettings {
    pub threshold_percent: Option<f64>,
    pub threshold_tokens: Option<u32>,
    pub base_window_tokens: Option<u32>,
    pub reserve_tokens: Option<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThresholdOverride {
    Percent(f64),
    Fixed(u32),
    BaseWindow(u32),
}
fn proportional(window: u32) -> u32 {
    window / 100 * 15 + window % 100 * 15 / 100
}
pub fn effective_reserve(window: u32, reserve: Option<u32>) -> u32 {
    proportional(window).max(reserve.unwrap_or(16_384))
}
pub fn budget_reserve(window: u32, reserve: Option<u32>) -> u32 {
    let effective = effective_reserve(window, reserve);
    let proportional = proportional(window).max(1);
    if effective >= window
        || (reserve.is_none() && effective >= window.saturating_sub(proportional))
    {
        proportional
    } else {
        effective
    }
}
pub fn prompt_budget(window: u32, reserve: Option<u32>) -> u32 {
    window
        .saturating_sub(1)
        .min(window.saturating_sub(budget_reserve(window, reserve)))
}
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Clamped percentage of a u32 window"
)]
pub fn resolve_threshold(window: u32, settings: &ThresholdSettings) -> u32 {
    if let Some(fixed) = settings.threshold_tokens.filter(|value| *value > 0) {
        return if fixed >= window {
            prompt_budget(window, settings.reserve_tokens)
        } else {
            fixed.max(1).min(window.saturating_sub(1))
        };
    }
    let window = settings
        .base_window_tokens
        .filter(|value| *value > 0)
        .map_or(window, |base| window.min(base));
    settings
        .threshold_percent
        .filter(|value| value.is_finite() && *value > 0.0)
        .map_or_else(
            || prompt_budget(window, settings.reserve_tokens),
            |percent| (f64::from(window) * percent.clamp(1.0, 99.0) / 100.0).floor() as u32,
        )
}
pub fn should_compact(context_tokens: u32, window: u32, threshold: u32) -> bool {
    window > 0 && context_tokens > threshold
}
pub fn apply_model_override(
    base: &ThresholdSettings,
    over: Option<ThresholdOverride>,
) -> ThresholdSettings {
    let mut result = base.clone();
    match over {
        Some(ThresholdOverride::BaseWindow(window)) => {
            result.base_window_tokens = Some(window);
            result.threshold_tokens = None;
        }
        Some(ThresholdOverride::Percent(percent)) => {
            result.threshold_percent = Some(percent);
            result.threshold_tokens = None;
        }
        Some(ThresholdOverride::Fixed(tokens)) => {
            result.threshold_percent = None;
            result.threshold_tokens = Some(tokens);
        }
        None => {}
    }
    result
}
pub fn apply_agent_override(
    base: &ThresholdSettings,
    over: Option<ThresholdOverride>,
) -> ThresholdSettings {
    apply_model_override(
        base,
        over.map(|value| match value {
            ThresholdOverride::BaseWindow(tokens) => ThresholdOverride::Fixed(tokens),
            other => other,
        }),
    )
}
/// The model's override: an exact key in either spelling (`provider/model`,
/// `provider:model`) beats every wildcard; otherwise the longest terminal-`*` prefix wins.
pub fn match_model_key<'a, V>(
    map: &'a BTreeMap<String, V>,
    provider: &str,
    model: &str,
) -> Option<&'a V> {
    matching_model_key(map, provider, model).map(|(_, value)| value)
}
/// Like [`match_model_key`], also returning the key that matched.
pub fn matching_model_key<'a, V>(
    map: &'a BTreeMap<String, V>,
    provider: &str,
    model: &str,
) -> Option<(&'a str, &'a V)> {
    let slash = format!("{provider}/{model}");
    let colon = format!("{provider}:{model}");
    [&slash, &colon]
        .into_iter()
        .find_map(|exact| map.get_key_value(exact.as_str()))
        .or_else(|| {
            map.iter()
                .filter_map(|(key, value)| {
                    let prefix = key.strip_suffix('*')?;
                    if prefix.contains('*') || !(prefix.contains('/') || prefix.contains(':')) {
                        return None;
                    }
                    (slash.starts_with(prefix) || colon.starts_with(prefix)).then_some((
                        prefix.len(),
                        key,
                        value,
                    ))
                })
                .max_by_key(|(length, _, _)| *length)
                .map(|(_, key, value)| (key, value))
        })
        .map(|(key, value)| (key.as_str(), value))
}
/// Parse typed model input; numeric agent input becomes fixed when applied.
pub fn parse_override(text: &str, allow_fixed: bool) -> Result<ThresholdOverride, String> {
    let text = text.trim();
    let invalid = || format!("Invalid compaction threshold: {text}");
    if let Some(percent) = text.strip_suffix('%') {
        let mut parts = percent.split('.');
        let whole = parts.next().unwrap_or_default();
        if whole.is_empty()
            || !whole.bytes().all(|c| c.is_ascii_digit())
            || parts
                .next()
                .is_some_and(|part| part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()))
            || parts.next().is_some()
        {
            return Err(invalid());
        }
        let value: f64 = percent.parse().map_err(|_| invalid())?;
        return if value.is_finite() && value > 0.0 && value <= 100.0 {
            Ok(ThresholdOverride::Percent(value))
        } else {
            Err(invalid())
        };
    }
    let fixed = text.starts_with(['f', 'F']);
    if fixed && !allow_fixed {
        return Err(invalid());
    }
    let number = if fixed { &text[1..] } else { text };
    let (digits, multiplier) = match number.as_bytes().last() {
        Some(b'k' | b'K') => (&number[..number.len() - 1], 1_000),
        Some(b'm' | b'M') => (&number[..number.len() - 1], 1_000_000),
        Some(b'b' | b'B') => (&number[..number.len() - 1], 1_000_000_000),
        _ => (number, 1),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return Err(invalid());
    }
    let value = digits
        .parse::<u32>()
        .ok()
        .and_then(|value| value.checked_mul(multiplier))
        .filter(|value| *value > 0)
        .ok_or_else(invalid)?;
    Ok(if fixed {
        ThresholdOverride::Fixed(value)
    } else {
        ThresholdOverride::BaseWindow(value)
    })
}
pub const RECOVERY_BAND: f64 = 0.8;
pub fn recovery_target(threshold: u32) -> u32 {
    threshold / 5 * 4 + threshold % 5 * 4 / 5
}
pub fn speculation_lead(threshold: u32) -> u32 {
    (threshold / 8).clamp(8_192, 32_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn threshold_reserve_parity() {
        for (window, reserve, base, percent, fixed, expected) in [
            (1_000_000, None, Some(400_000), None, None, 340_000),
            (1_000_000, None, Some(400_000), Some(80.0), None, 320_000),
            (1_000_000, None, Some(400_000), None, Some(500_000), 500_000),
            (196_608, None, None, None, Some(300_000), 167_117),
            (196_608, None, None, None, Some(196_608), 167_117),
            (196_608, None, None, None, Some(196_607), 196_607),
            (196_608, None, None, None, Some(167_118), 167_118),
            (1_000_000, None, None, None, Some(300_000), 300_000),
            (200_000, Some(50_000), None, None, Some(250_000), 150_000),
            (200_000, Some(50_000), None, None, Some(180_000), 180_000),
            (16_385, None, None, None, Some(20_000), 13_928),
            (16_385, None, None, None, Some(10_000), 10_000),
            (2, None, None, None, Some(5), 1),
            (1, None, None, None, Some(5), 0),
            (19_000, Some(16_384), None, None, None, 2_616),
            (19_000, None, None, None, None, 16_150),
            (6, None, None, None, None, 5),
            (1, None, None, None, None, 0),
            (16_385, Some(90_000), None, None, None, 13_928),
            (32_768, None, None, None, None, 16_384),
            (131_072, None, None, None, None, 111_412),
            (101, None, None, Some(100.0), None, 99),
            (101, None, None, Some(0.1), None, 1),
        ] {
            let settings = ThresholdSettings {
                reserve_tokens: reserve,
                base_window_tokens: base,
                threshold_percent: percent,
                threshold_tokens: fixed,
            };
            assert_eq!(
                resolve_threshold(window, &settings),
                expected,
                "{settings:?} at {window}"
            );
            assert!(!should_compact(expected, window, expected));
            assert_eq!(should_compact(expected + 1, window, expected), window > 0);
        }
        assert_eq!(effective_reserve(19_000, None), 16_384);
        assert_eq!(budget_reserve(19_000, None), 2_850);
        assert!(!should_compact(10, 0, 0));
    }
    #[test]
    fn overrides_replace_only_the_intended_fields() {
        let base = ThresholdSettings {
            threshold_tokens: Some(900),
            threshold_percent: Some(80.0),
            reserve_tokens: None,
            base_window_tokens: Some(1_000),
        };
        let model = apply_model_override(&base, Some(ThresholdOverride::BaseWindow(400)));
        assert_eq!(
            (
                model.threshold_tokens,
                model.threshold_percent,
                model.base_window_tokens,
                model.reserve_tokens
            ),
            (None, Some(80.0), Some(400), None)
        );
        assert_eq!(
            apply_agent_override(&base, Some(ThresholdOverride::BaseWindow(400))).threshold_tokens,
            Some(400)
        );
        for (text, allowed, expected) in [
            (" 80% ", true, Some(ThresholdOverride::Percent(80.0))),
            ("F32k", true, Some(ThresholdOverride::Fixed(32_000))),
            ("400000", true, Some(ThresholdOverride::BaseWindow(400_000))),
            ("f10", false, None),
            ("0", true, None),
            ("1.2k", true, None),
            ("101%", true, None),
            ("+2%", true, None),
        ] {
            assert_eq!(parse_override(text, allowed).ok(), expected);
        }
        let map = BTreeMap::from([
            ("provider/model*".into(), 1),
            ("provider/model-long*".into(), 2),
            ("provider:model-long-exact".into(), 3),
        ]);
        assert_eq!(
            match_model_key(&map, "provider", "model-long-exact"),
            Some(&3)
        );
        assert_eq!(
            match_model_key(&map, "provider", "model-long-other"),
            Some(&2)
        );
    }
    #[test]
    fn exact_key_in_either_spelling_beats_any_wildcard() {
        let map = BTreeMap::from([
            ("p/m".to_owned(), 1),
            ("p:m".to_owned(), 2),
            ("p/m*".to_owned(), 3),
            ("q:m".to_owned(), 4),
            ("q/m*".to_owned(), 5),
            ("q/*".to_owned(), 6),
        ]);
        for (provider, model, expected) in [
            ("p", "m", Some(("p/m", 1))),
            ("q", "m", Some(("q:m", 4))),
            ("q", "mx", Some(("q/m*", 5))),
            ("q", "other", Some(("q/*", 6))),
            ("r", "m", None),
        ] {
            assert_eq!(
                matching_model_key(&map, provider, model).map(|(key, value)| (key, *value)),
                expected,
                "{provider}/{model}"
            );
        }
    }
}
