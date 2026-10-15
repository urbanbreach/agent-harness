//! Tolerant numeric grammar shared by the compaction settings.
//!
//! JSON numbers up to `u64` are accepted for token quantities and saturate to `u32::MAX`.
//! Whole-valued floats count as integers. `-1` is the "unset" sentinel only where a field
//! documents it. The idle and retained-budget fields accept any finite number: the value is
//! floored, then clamped into the field's range, so negatives become zero.
use super::*;
use num_traits::ToPrimitive;
use serde::de::Error;
use serde_json::Value;

fn whole(value: &Value) -> Option<i128> {
    let number = value.as_number()?;
    if let Some(unsigned) = number.as_u64() {
        return Some(i128::from(unsigned));
    }
    if let Some(signed) = number.as_i64() {
        return Some(i128::from(signed));
    }
    let float = number.as_f64().filter(|float| float.is_finite())?;
    if float.fract() == 0.0 {
        float.to_i128()
    } else {
        None
    }
}

fn saturate(value: i128) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// A positive whole number, saturating at `u32::MAX`.
pub(super) fn positive(value: &Value) -> Option<NonZeroU32> {
    whole(value)
        .filter(|number| *number >= 1)
        .and_then(|number| NonZeroU32::new(saturate(number)))
}

fn count(value: &Value, field: &str) -> Result<u32, String> {
    whole(value)
        .filter(|number| *number >= 0)
        .map(saturate)
        .ok_or_else(|| format!("{field} must be a non-negative whole number"))
}

/// A non-negative whole number of tokens.
pub(super) fn token_count<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    count(&Value::deserialize(deserializer)?, "token counts").map_err(D::Error::custom)
}

fn unset_or(value: &Value, field: &str, minimum: i128) -> Result<Option<u32>, String> {
    if value.is_null() {
        return Ok(None);
    }
    match whole(value) {
        Some(-1) => Ok(None),
        Some(number) if number >= minimum => Ok(Some(saturate(number))),
        _ => Err(format!(
            "{field} must be a {} whole number or -1 to leave it unset",
            if minimum == 0 {
                "non-negative"
            } else {
                "positive"
            }
        )),
    }
}

/// `null` and `-1` mean unset; otherwise a non-negative whole number.
pub(super) fn reserve_tokens<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    unset_or(&Value::deserialize(deserializer)?, "reserve_tokens", 0).map_err(D::Error::custom)
}

/// `null` and `-1` mean unset; otherwise a positive whole number.
pub(super) fn base_window_tokens<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    unset_or(&Value::deserialize(deserializer)?, "base_window_tokens", 1).map_err(D::Error::custom)
}

/// `null` and `-1` mean unset; otherwise a positive whole number.
pub(super) fn threshold_tokens<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NonZeroU32>, D::Error> {
    unset_or(&Value::deserialize(deserializer)?, "threshold_tokens", 1)
        .map(|tokens| tokens.and_then(NonZeroU32::new))
        .map_err(D::Error::custom)
}

/// `null` and `-1` mean unset; otherwise a percentage above 0 and at most 100.
pub(super) fn threshold_percent<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<CompactionThresholdPercent>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    if value.is_null() || whole(&value) == Some(-1) {
        return Ok(None);
    }
    CompactionThresholdPercent::from_value(&value)
        .map(Some)
        .map_err(D::Error::custom)
}

/// Floor a finite number and clamp it into `0..=max`.
fn clamped(value: &Value, field: &str, max: u64) -> Result<u64, String> {
    let number = value
        .as_number()
        .ok_or_else(|| format!("{field} must be a number"))?;
    if let Some(unsigned) = number.as_u64() {
        return Ok(unsigned.min(max));
    }
    if number.as_i64().is_some() {
        return Ok(0);
    }
    let float = number
        .as_f64()
        .filter(|float| float.is_finite())
        .ok_or_else(|| format!("{field} must be a finite number"))?;
    if float <= 0.0 {
        return Ok(0);
    }
    Ok(float
        .floor()
        .to_u64()
        .map_or(max, |floored| floored.min(max)))
}

/// Finite number floored and clamped into `u32`.
pub(super) fn clamped_u32<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u32, D::Error> {
    let value = Value::deserialize(deserializer)?;
    clamped(&value, "value", u64::from(u32::MAX))
        .map(|number| saturate(i128::from(number)))
        .map_err(D::Error::custom)
}

/// Finite number floored and clamped into `u64`.
pub(super) fn clamped_u64<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u64, D::Error> {
    clamped(&Value::deserialize(deserializer)?, "value", u64::MAX).map_err(D::Error::custom)
}

/// Empty or whitespace-only endpoints are absent.
pub(super) fn endpoint<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty()))
}

/// Unknown entries are discarded, the first duplicate wins and anything that is not an array
/// falls back to the default order.
pub(super) fn method_order<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<CompactionMethod>, D::Error> {
    Ok(CompactionMethod::parse_order(&Value::deserialize(
        deserializer,
    )?))
}
