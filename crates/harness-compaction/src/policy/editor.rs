//! Pure helpers behind the model-hub threshold editor: display formatting, edit planning
//! and the preview line. They hold no state; the caller owns settings and persistence.
use super::{
    apply_model_override, matching_model_key, parse_override, resolve_threshold, ThresholdOverride,
    ThresholdSettings,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Largest exactly divisible `B`, `M` or `K` suffix, else plain digits.
fn suffixed(tokens: u32) -> String {
    for (scale, suffix) in [(1_000_000_000, 'B'), (1_000_000, 'M'), (1_000, 'K')] {
        if tokens >= scale && tokens.is_multiple_of(scale) {
            return format!("{}{suffix}", tokens / scale);
        }
    }
    tokens.to_string()
}

/// How the editor shows an override. The text parses back to the same override with
/// `parse_override(text, true)`. Persisted spellings differ: see [`plan_override`].
pub fn format_override(over: &ThresholdOverride) -> String {
    match over {
        ThresholdOverride::Percent(percent) => format!("{percent}%"),
        ThresholdOverride::Fixed(tokens) => format!("f{}", suffixed(*tokens)),
        ThresholdOverride::BaseWindow(tokens) => suffixed(*tokens),
    }
}

/// Something the editor should flag without refusing the edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideWarning {
    /// A base window at or above the model window changes nothing.
    BaseNotBelowWindow,
}

/// What saving one editor entry does to the persisted model map.
#[derive(Debug, Clone, PartialEq)]
pub struct OverridePlan {
    /// The value to store under `key`; `None` removes the key from the map.
    pub write: Option<Value>,
    pub key: String,
    pub warnings: Vec<OverrideWarning>,
    /// After a reset, the other key (a wildcard prefix or the other spelling) that still
    /// decides this model's threshold.
    pub inherits: Option<String>,
}

/// Plans an edit of `key` in the effective model map `models`.
///
/// Empty or whitespace input resets: the key is removed and `inherits` names the key that
/// still matches the model, if any. Otherwise the input is parsed with the typed grammar.
/// A fixed trigger above `window` is refused; a base window at or above it is flagged.
/// Persisted forms: percent `"80%"`, fixed `"f150000"` (digits only; the suffix spelling is
/// display-only), base window a JSON number.
pub fn plan_override(
    models: &BTreeMap<String, ThresholdOverride>,
    key: &str,
    input: &str,
    window: Option<u32>,
) -> Result<OverridePlan, String> {
    let window = window.filter(|window| *window > 0);
    if input.trim().is_empty() {
        let rest: BTreeMap<String, ThresholdOverride> = models
            .iter()
            .filter(|(candidate, _)| candidate.as_str() != key)
            .map(|(candidate, value)| (candidate.clone(), *value))
            .collect();
        let inherits = key
            .find(['/', ':'])
            .filter(|_| !key.ends_with('*'))
            .and_then(|at| matching_model_key(&rest, &key[..at], &key[at + 1..]))
            .map(|(matched, _)| matched.to_owned());
        return Ok(OverridePlan {
            write: None,
            key: key.to_owned(),
            warnings: Vec::new(),
            inherits,
        });
    }
    let over = parse_override(input, true)?;
    let mut warnings = Vec::new();
    let write = match over {
        ThresholdOverride::Percent(_) => json!(format_override(&over)),
        ThresholdOverride::Fixed(tokens) => {
            if let Some(window) = window.filter(|window| tokens > *window) {
                return Err(format!(
                    "Fixed trigger {tokens} is above the model window of {window} tokens"
                ));
            }
            json!(format!("f{tokens}"))
        }
        ThresholdOverride::BaseWindow(tokens) => {
            if window.is_some_and(|window| tokens >= window) {
                warnings.push(OverrideWarning::BaseNotBelowWindow);
            }
            json!(tokens)
        }
    };
    Ok(OverridePlan {
        write: Some(write),
        key: key.to_owned(),
        warnings,
        inherits: None,
    })
}

/// Where a previewed trigger comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewOrigin {
    /// A fixed trigger as configured, even when the window caps it in practice.
    ConfiguredFixed,
    /// Scaled from the model window or a base window.
    ScaledFromBase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThresholdPreview {
    pub tokens: u32,
    pub origin: PreviewOrigin,
}

/// The trigger an override would give on a model, or `None` when the window is unknown or
/// zero, or the override itself is invalid (non-positive or out-of-range values).
pub fn preview(
    window: Option<u32>,
    over: Option<&ThresholdOverride>,
    settings: &ThresholdSettings,
) -> Option<ThresholdPreview> {
    let window = window.filter(|window| *window > 0)?;
    let valid = match over {
        None => true,
        Some(ThresholdOverride::Percent(percent)) => {
            percent.is_finite() && *percent > 0.0 && *percent <= 100.0
        }
        Some(ThresholdOverride::Fixed(tokens) | ThresholdOverride::BaseWindow(tokens)) => {
            *tokens > 0
        }
    };
    if !valid {
        return None;
    }
    let effective = apply_model_override(settings, over.copied());
    Some(
        match effective.threshold_tokens.filter(|tokens| *tokens > 0) {
            Some(tokens) => ThresholdPreview {
                tokens,
                origin: PreviewOrigin::ConfiguredFixed,
            },
            None => ThresholdPreview {
                tokens: resolve_threshold(window, &effective),
                origin: PreviewOrigin::ScaledFromBase,
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_round_trips_through_the_parser() {
        for (over, text) in [
            (ThresholdOverride::BaseWindow(400_000), "400K"),
            (ThresholdOverride::BaseWindow(1_000_000), "1M"),
            (ThresholdOverride::BaseWindow(2_500_000), "2500K"),
            (ThresholdOverride::BaseWindow(3_000_000_000), "3B"),
            (ThresholdOverride::BaseWindow(1_234), "1234"),
            (ThresholdOverride::Fixed(150_000), "f150K"),
            (ThresholdOverride::Fixed(999), "f999"),
            (ThresholdOverride::Percent(80.0), "80%"),
            (ThresholdOverride::Percent(80.5), "80.5%"),
            (ThresholdOverride::Percent(0.5), "0.5%"),
        ] {
            assert_eq!(format_override(&over), text);
            assert_eq!(parse_override(text, true), Ok(over), "{text}");
        }
    }

    #[test]
    fn planning_validates_fit_and_resets_to_the_remaining_match() {
        let models = BTreeMap::from([
            ("p/m".to_owned(), ThresholdOverride::Percent(70.0)),
            ("p/*".to_owned(), ThresholdOverride::Percent(60.0)),
            ("p/m-*".to_owned(), ThresholdOverride::Percent(65.0)),
            ("q:n".to_owned(), ThresholdOverride::Fixed(10)),
        ]);
        for (key, input, window, write, warnings, inherits) in [
            ("p/m", " 80% ", Some(100), Some(json!("80%")), vec![], None),
            (
                "p/m",
                "f150k",
                Some(200_000),
                Some(json!("f150000")),
                vec![],
                None,
            ),
            (
                "p/m",
                "400k",
                Some(1_000_000),
                Some(json!(400_000)),
                vec![],
                None,
            ),
            (
                "p/m",
                "200000",
                Some(200_000),
                Some(json!(200_000)),
                vec![OverrideWarning::BaseNotBelowWindow],
                None,
            ),
            (
                "p/m",
                "f200000",
                Some(200_000),
                Some(json!("f200000")),
                vec![],
                None,
            ),
            ("p/m", "f300000", None, Some(json!("f300000")), vec![], None),
            ("p/m", "  ", Some(100), None, vec![], Some("p/*")),
            ("p/m-x", "", None, None, vec![], Some("p/m-*")),
            ("q:n", "", None, None, vec![], None),
        ] {
            let plan = plan_override(&models, key, input, window).expect("plan");
            assert_eq!(plan.key, key);
            assert_eq!(plan.write, write, "{key} {input}");
            assert_eq!(plan.warnings, warnings, "{key} {input}");
            assert_eq!(plan.inherits.as_deref(), inherits, "{key} {input}");
        }
        for (input, window) in [("f200001", Some(200_000)), ("1.2k", None), ("0", None)] {
            assert!(
                plan_override(&models, "p/m", input, window).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn preview_hides_unknown_windows_and_reports_origin() {
        let settings = ThresholdSettings::default();
        for (window, over, expected) in [
            (
                Some(1_000_000),
                Some(ThresholdOverride::BaseWindow(400_000)),
                Some((340_000, PreviewOrigin::ScaledFromBase)),
            ),
            (
                Some(1_000_000),
                Some(ThresholdOverride::Percent(80.0)),
                Some((800_000, PreviewOrigin::ScaledFromBase)),
            ),
            (
                Some(200_000),
                Some(ThresholdOverride::Fixed(90_000)),
                Some((90_000, PreviewOrigin::ConfiguredFixed)),
            ),
            (
                Some(32_000),
                Some(ThresholdOverride::Fixed(90_000)),
                Some((90_000, PreviewOrigin::ConfiguredFixed)),
            ),
            (
                Some(200_000),
                None,
                Some((170_000, PreviewOrigin::ScaledFromBase)),
            ),
            (None, Some(ThresholdOverride::Percent(80.0)), None),
            (Some(0), None, None),
            (Some(100), Some(ThresholdOverride::Percent(0.0)), None),
            (Some(100), Some(ThresholdOverride::Percent(101.0)), None),
            (Some(100), Some(ThresholdOverride::Fixed(0)), None),
        ] {
            let got =
                preview(window, over.as_ref(), &settings).map(|value| (value.tokens, value.origin));
            assert_eq!(got, expected, "{window:?} {over:?}");
        }
    }
}
