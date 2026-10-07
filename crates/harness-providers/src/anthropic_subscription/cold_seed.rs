//! Cold-seed budget (senpi `cold-seed-budget.ts`).
//!
//! A cold-seed (flatten/bootstrap) re-sends the
//! history as ONE user message, which Claude Code cannot compact, so an over-budget re-send is
//! refused before dispatch and handed to harness compaction instead.
use super::errors::{LaneError, OWN_REFUSAL_PREFIX};
use super::prompt::{serialized_payload_bytes, LaneContext};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{LazyLock, Mutex},
};

/// Claude's tokenizer spends at least one token per ~4 UTF-8 bytes on realistic text.
const UTF8_BYTES_PER_TOKEN_FLOOR: usize = 4;
/// A ratio above this was not about the payload (a provider-side limit or a bad parse).
const MAX_CALIBRATION_RATIO: f64 = 8.0;
const MAX_CALIBRATED_SESSIONS: usize = 256;

pub fn estimate_cold_seed_tokens(context: &LaneContext, blocks: &[Value]) -> u64 {
    let tools = context
        .tools
        .as_ref()
        .map_or_else(|| "[]".to_owned(), |tools| {
            Value::Array(
                tools
                    .iter()
                    .map(|t| serde_json::json!({"name": t.function_name, "description": t.description, "parameters": t.parameters}))
                    .collect(),
            )
            .to_string()
        });
    let fixed = context.system_prompt.as_deref().map_or(0, str::len) + tools.len();
    u64::try_from((fixed + serialized_payload_bytes(blocks)).div_ceil(UTF8_BYTES_PER_TOKEN_FLOOR))
        .unwrap_or(u64::MAX)
}

static CALIBRATION: LazyLock<Mutex<VecDeque<(String, f64)>>> = LazyLock::new(Mutex::default);

/// Tokens the API counts per bytes/4-estimated token, learned from this session's rejected
/// cold-seeds; 1 until one is seen.
pub fn cold_seed_calibration(session_id: Option<&str>) -> f64 {
    let Some(session_id) = session_id else {
        return 1.0;
    };
    CALIBRATION
        .lock()
        .ok()
        .and_then(|map| map.iter().find(|(id, _)| id == session_id).map(|(_, r)| *r))
        .unwrap_or(1.0)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "token counts are far below f64 precision"
)]
pub fn raise_calibration(session_id: &str, estimated: u64, reported: u64) {
    if estimated == 0 || reported == 0 {
        return;
    }
    let ratio = (reported as f64 / estimated as f64).min(MAX_CALIBRATION_RATIO);
    if ratio <= cold_seed_calibration(Some(session_id)) {
        return;
    }
    if let Ok(mut map) = CALIBRATION.lock() {
        map.retain(|(id, _)| id != session_id);
        map.push_back((session_id.into(), ratio));
        if map.len() > MAX_CALIBRATED_SESSIONS {
            map.pop_front();
        }
    }
}

pub fn forget_cold_seed_calibration(session_id: Option<&str>) {
    if let Ok(mut map) = CALIBRATION.lock() {
        match session_id {
            Some(id) => map.retain(|(entry, _)| entry != id),
            None => map.clear(),
        }
    }
}

static REPORTED: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)~?(\d[\d,]*)\s+tokens?\s*\(limit\s+(\d[\d,]*)\)",
        r"(?i)(\d[\d,]*)\s+tokens?\s*>\s*(\d[\d,]*)\s+maximum",
        r"(?i)about\s+(\d[\d,]*)\s+tokens?,\s*limit\s+(\d[\d,]*)",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// The counts a provider rejection reports: `(reported tokens, reported limit)`.
pub fn parse_reported_overflow_tokens(message: &str) -> Option<(u64, u64)> {
    let captures = REPORTED.iter().find_map(|re| re.captures(message))?;
    let number = |i: usize| {
        captures
            .get(i)?
            .as_str()
            .replace(',', "")
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
    };
    Some((number(1)?, number(2)?))
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "bounded token estimates"
)]
pub fn cold_seed_overflow(
    context_window: Option<u64>,
    estimated: u64,
    calibration: f64,
) -> Option<LaneError> {
    let window = context_window.filter(|w| *w > 0)?;
    let calibrated = (estimated as f64 * calibration.max(1.0)).ceil() as u64;
    (calibrated > window).then_some(LaneError::ColdSeedOverflow {
        estimated_tokens: calibrated,
        context_window: window,
    })
}

/// After a cold-seed failed on context size: learn how far bytes/4 under-counted.
pub fn learn_from_overflow(error_message: &str, estimated: Option<u64>, session_id: Option<&str>) {
    if error_message.starts_with(OWN_REFUSAL_PREFIX) {
        return;
    }
    if let (Some((reported, _)), Some(estimated), Some(session_id)) = (
        parse_reported_overflow_tokens(error_message),
        estimated,
        session_id,
    ) {
        raise_calibration(session_id, estimated, reported);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_learns_from_reported_counts_only() {
        assert_eq!(
            parse_reported_overflow_tokens("the request is ~250,000 tokens (limit 200000)"),
            Some((250_000, 200_000))
        );
        assert_eq!(
            parse_reported_overflow_tokens("prompt is too long: 210000 tokens > 200000 maximum"),
            Some((210_000, 200_000))
        );
        learn_from_overflow(
            "prompt is too long: 300 tokens > 200 maximum",
            Some(100),
            Some("calibrated"),
        );
        assert!((cold_seed_calibration(Some("calibrated")) - 3.0).abs() < f64::EPSILON);
        learn_from_overflow(
            &format!("{OWN_REFUSAL_PREFIX} (about 900 tokens, limit 200)"),
            Some(100),
            Some("own"),
        );
        assert!((cold_seed_calibration(Some("own")) - 1.0).abs() < f64::EPSILON);
        assert!(cold_seed_overflow(Some(250), 100, 3.0).is_some());
        assert!(cold_seed_overflow(Some(350), 100, 3.0).is_none());
    }
}
