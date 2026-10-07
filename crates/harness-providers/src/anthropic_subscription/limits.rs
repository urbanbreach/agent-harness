//! Usage-limit reset times (senpi `credential-pool/reset-time.ts`).
//!
//! When a spent account's usage limit resets, read from the failure text: a JSON reset
//! field or reset prose. Milliseconds from
//! `now` (0 for a reset already past), or `None` so the caller keeps its default cooldown.
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const MINUTE_MS: f64 = 60_000.0;
const HOUR_MS: f64 = 60.0 * MINUTE_MS;
const DAY_MS: f64 = 24.0 * HOUR_MS;
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

static RELATIVE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:resets?|try again|retry)\s+in\s+~?((?:\d+(?:\.\d+)?\s*(?:days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b(?:\s*,?\s*(?:and\s+)?)?)+)").ok()
});
static RELATIVE_PART: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?i)(\d+(?:\.\d+)?)\s*(days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b")
        .ok()
});
static CLOCK: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:resets?|try again)\s+(?:at\s+|on\s+)?(?:([a-z]{3})[a-z]*\.?\s+(\d{1,2}),?\s+(?:at\s+)?)?(\d{1,2})(?::(\d{2}))?\s*([ap])\.?\s?m\b\.?(?:\s*\(([A-Za-z_]+(?:/[A-Za-z0-9_+-]+)*)\))?").ok()
});

pub fn usage_limit_reset_ms(text: &str, now_ms: i64) -> Option<i64> {
    from_json_fields(text, now_ms)
        .or_else(|| from_relative_prose(text))
        .or_else(|| from_clock_prose(text, now_ms))
}

fn from_json_fields(text: &str, now_ms: i64) -> Option<i64> {
    let start = text.find('{')?;
    let parsed: Value = serde_json::from_str(&text[start..]).ok()?;
    let mut found: Option<i64> = None;
    walk(&parsed, &mut |key, value| {
        let ms = match key {
            "resets_at" => absolute_field_ms(value, now_ms),
            "reset_after_seconds" => seconds_field_ms(value),
            _ => None,
        };
        if let Some(ms) = ms
            && found.is_none_or(|f| ms > f)
        {
            found = Some(ms);
        }
    });
    found
}

fn walk(value: &Value, visit: &mut dyn FnMut(&str, &Value)) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| walk(item, visit)),
        Value::Object(map) => {
            for (key, entry) in map {
                visit(key, entry);
                walk(entry, visit);
            }
        }
        _ => {}
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "epoch milliseconds"
)]
fn absolute_field_ms(value: &Value, now_ms: i64) -> Option<i64> {
    if let Some(number) = value.as_f64().filter(|n| n.is_finite() && *n > 0.0) {
        let epoch = if number < 1e12 {
            number * 1000.0
        } else {
            number
        };
        return Some((epoch - now_ms as f64).ceil().max(0.0) as i64);
    }
    let text = value.as_str()?;
    let shape = Regex::new(r"\d{4}-\d{2}-\d{2}T").ok()?;
    if !shape.is_match(text) {
        return None;
    }
    let epoch = text.parse::<jiff::Timestamp>().ok()?.as_millisecond();
    Some((epoch - now_ms).max(0))
}

#[allow(clippy::cast_possible_truncation, reason = "bounded durations")]
fn seconds_field_ms(value: &Value) -> Option<i64> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| (n * 1000.0).ceil() as i64)
}

fn unit_ms(unit: &str) -> f64 {
    let u = unit.to_lowercase();
    if u.starts_with('d') {
        DAY_MS
    } else if u.starts_with('h') {
        HOUR_MS
    } else if u == "s" || u.starts_with("sec") {
        1000.0
    } else {
        MINUTE_MS
    }
}

#[allow(clippy::cast_possible_truncation, reason = "bounded durations")]
fn from_relative_prose(text: &str) -> Option<i64> {
    let phrase = RELATIVE.as_ref()?.captures(text)?.get(1)?.as_str();
    let total: f64 = RELATIVE_PART
        .as_ref()?
        .captures_iter(phrase)
        .map(|c| c[1].parse::<f64>().unwrap_or(0.0) * unit_ms(&c[2]))
        .sum();
    Some(total.ceil() as i64)
}

fn zoned_epoch(
    zone: &jiff::tz::TimeZone,
    year: i16,
    month: i8,
    day: i32,
    hour: i8,
    minute: i8,
) -> Option<i64> {
    // `Date.UTC` semantics: an out-of-range day rolls into the next month.
    let first = jiff::civil::date(year, month, 1);
    let date = first
        .checked_add(jiff::Span::new().days(i64::from(day) - 1))
        .ok()?;
    let datetime = date.at(hour, minute, 0, 0);
    Some(zone.to_timestamp(datetime).ok()?.as_millisecond())
}

fn from_clock_prose(text: &str, now_ms: i64) -> Option<i64> {
    let captures = CLOCK.as_ref()?.captures(text)?;
    let hour12: i8 = captures.get(3)?.as_str().parse().ok()?;
    let minute: i8 = captures
        .get(4)
        .map_or(Some(0), |m| m.as_str().parse().ok())?;
    if !(1..=12).contains(&hour12) || minute > 59 {
        return None;
    }
    let pm = captures.get(5)?.as_str().eq_ignore_ascii_case("p");
    let hour = hour12 % 12 + if pm { 12 } else { 0 };
    let zone = match captures.get(6) {
        Some(name) => jiff::tz::TimeZone::get(name.as_str()).ok()?,
        None => jiff::tz::TimeZone::system(),
    };
    let today = jiff::Timestamp::from_millisecond(now_ms)
        .ok()?
        .to_zoned(zone.clone());
    if let Some(month_name) = captures.get(1) {
        let month = MONTHS
            .iter()
            .position(|m| month_name.as_str().eq_ignore_ascii_case(m))?;
        let month = i8::try_from(month + 1).ok()?;
        let day: i32 = captures.get(2)?.as_str().parse().ok()?;
        if !(1..=31).contains(&day) {
            return None;
        }
        let mut target = zoned_epoch(&zone, today.year(), month, day, hour, minute)?;
        if target <= now_ms {
            target = zoned_epoch(&zone, today.year() + 1, month, day, hour, minute)?;
        }
        return Some(target - now_ms);
    }
    let mut target = zoned_epoch(
        &zone,
        today.year(),
        today.month(),
        i32::from(today.day()),
        hour,
        minute,
    )?;
    if target <= now_ms {
        target = zoned_epoch(
            &zone,
            today.year(),
            today.month(),
            i32::from(today.day()) + 1,
            hour,
            minute,
        )?;
    }
    Some(target - now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_times_come_from_json_relative_and_clock_prose() {
        // 2026-10-07T12:00:00Z
        let now = 1_791_374_400_000;
        assert_eq!(
            usage_limit_reset_ms(r#"429 {"error":{"resets_at":1791378000}}"#, now),
            Some(3_600_000)
        );
        assert_eq!(
            usage_limit_reset_ms("limit hit; resets in 2h 30m", now),
            Some(9_000_000)
        );
        // 12:00Z is 21:00 in Seoul; 5am Seoul is 20:00Z.
        assert_eq!(
            usage_limit_reset_ms(
                "You've hit your weekly limit · resets 5am (Asia/Seoul)",
                now
            ),
            Some(8 * 3_600_000)
        );
        assert_eq!(
            usage_limit_reset_ms("resets Oct 8, 9am (UTC)", now),
            Some(21 * 3_600_000)
        );
        assert_eq!(usage_limit_reset_ms("no reset here", now), None);
    }
}
