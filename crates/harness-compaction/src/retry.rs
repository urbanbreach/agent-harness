//! Clock-free retry delays. The caller owns classification, waiting and cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Total attempts and millisecond delay limits for an outer retry loop.
pub struct RetryPolicy {
    pub attempts: u32,
    pub base_ms: u64,
    pub max_wait_ms: u64,
    pub cap_ms: u64,
}
/// Default three-attempt summary retry policy.
pub const ONESHOT: RetryPolicy = RetryPolicy {
    attempts: 3,
    base_ms: 500,
    max_wait_ms: 30_000,
    cap_ms: 8000,
};
/// An automatic summary wait longer than this advances to the next candidate when one
/// remains; only the last candidate waits it out.
pub const ADVANCE_CUTOFF_MS: u64 = 30_000;

/// Delay before retrying the same candidate in the automatic loop: zero-based
/// `base * 2^attempt`, raised to the retry-after hint, with no generic ceiling.
pub fn automatic_delay_ms(base_ms: u64, attempt: u32, retry_after_ms: Option<u64>) -> u64 {
    base_ms
        .saturating_mul(1_u64.checked_shl(attempt).unwrap_or(u64::MAX))
        .max(retry_after_ms.unwrap_or(0))
}

#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Millisecond jitter is deliberately rounded and saturates at the integer boundary"
)]
/// Return jittered one-based exponential delay, or reject an excessive server hint.
pub fn backoff_ms(
    policy: &RetryPolicy,
    attempt: u32,
    jitter_unit: f64,
    retry_after_ms: Option<u64>,
) -> Option<u64> {
    let hint = retry_after_ms.unwrap_or(0);
    if hint > policy.max_wait_ms {
        return None;
    }
    let factor = 1_u64
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u64::MAX);
    let capped = policy.base_ms.saturating_mul(factor).min(policy.cap_ms);
    let jitter = if jitter_unit.is_finite() {
        jitter_unit.clamp(0.0, 1.0 - f64::EPSILON)
    } else {
        0.0
    };
    let delay = (capped as f64 * (0.75 + jitter * 0.25)).round() as u64;
    Some(delay.max(hint).min(policy.max_wait_ms))
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Finite positive suffix milliseconds are rounded upward with saturating float conversion"
)]
/// Extract the largest positive decimal retry-after-ms token, rounded upward.
pub fn parse_retry_after_suffix(message: &str) -> Option<u64> {
    message
        .split_whitespace()
        .filter_map(|part| {
            let number = part.strip_prefix("retry-after-ms=")?;
            // Decimal syntax only: exclude signs, exponents, NaN and infinities.
            let mut dot = false;
            let mut digits = false;
            for byte in number.bytes() {
                if byte.is_ascii_digit() {
                    digits = true;
                } else if byte == b'.' && !dot {
                    dot = true;
                } else {
                    return None;
                }
            }
            if !digits {
                return None;
            }
            let value: f64 = number.parse().ok()?;
            (value.is_finite() && value > 0.0).then(|| value.ceil() as u64)
        })
        .max()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jitter_cap_and_server_hints() {
        for (attempt, jitter, hint, expected) in [
            (1, 0.0, None, Some(375)),
            (1, 0.5, None, Some(438)),
            (2, 0.0, None, Some(750)),
            (10, 0.0, None, Some(6000)),
            (1, 0.0, Some(1000), Some(1000)),
            (1, 0.0, Some(31_000), None),
        ] {
            assert_eq!(backoff_ms(&ONESHOT, attempt, jitter, hint), expected);
        }
    }
    #[test]
    fn automatic_delay_doubles_from_base_and_follows_larger_hints() {
        for (attempt, hint, expected) in [
            (0, None, 500),
            (3, None, 4000),
            (12, None, 2_048_000),
            (0, Some(0), 500),
            (1, Some(31_000), 31_000),
        ] {
            assert_eq!(automatic_delay_ms(500, attempt, hint), expected);
        }
        assert_eq!(automatic_delay_ms(500, 90, None), u64::MAX);
    }

    #[test]
    fn retry_suffix_requires_a_whitespace_delimited_positive_decimal() {
        for (text, expected) in [
            ("failed retry-after-ms=1.2", Some(2)),
            ("retry-after-ms=0", None),
            ("xretry-after-ms=3", None),
            ("retry-after-ms=-2", None),
            ("retry-after-ms=NaN", None),
            ("retry-after-ms=2,", None),
            ("retry-after-ms=.5", Some(1)),
            ("retry-after-ms=2 retry-after-ms=4", Some(4)),
        ] {
            assert_eq!(parse_retry_after_suffix(text), expected);
        }
    }
}
