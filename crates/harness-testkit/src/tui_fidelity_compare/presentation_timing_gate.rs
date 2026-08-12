use super::error::ComparatorError;
use super::presentation_timing::{median_nonzero, PresentationTimingMetrics};
use super::timing::TimingDefect;

pub const P95_LIMIT_PERCENT: u64 = 110;
pub const MAX_GAP_MULTIPLIER: u64 = 2;

pub fn compare_presentation_timing(
    reference: &PresentationTimingMetrics,
    candidate: &PresentationTimingMetrics,
) -> Result<(), ComparatorError> {
    let mut defects = Vec::new();
    match (
        p95(&reference.external_send_to_changed_observation_micros),
        p95(&candidate.external_send_to_changed_observation_micros),
    ) {
        (Some(reference_p95), Some(candidate_p95))
            if candidate_p95.saturating_mul(100)
                > reference_p95.saturating_mul(P95_LIMIT_PERCENT) =>
        {
            defects.push(TimingDefect {
                reason: "external_p95".to_owned(),
                expected: format!("<= {P95_LIMIT_PERCENT}% of {reference_p95}us"),
                observed: format!("{candidate_p95}us"),
            });
        }
        (None, _) | (_, None) => defects.push(TimingDefect {
            reason: "empty_external_latency".to_owned(),
            expected: "mapped action latency samples".to_owned(),
            observed: "empty".to_owned(),
        }),
        _ => {}
    }
    check_intervals(
        "reference_external",
        &reference.external_observation_intervals_micros,
        reference.external_cadence_micros,
        &mut defects,
    );
    check_intervals(
        "candidate_external",
        &candidate.external_observation_intervals_micros,
        candidate.external_cadence_micros,
        &mut defects,
    );
    if let Some(native) = &candidate.native {
        check_intervals(
            "candidate_native",
            &native.completed_write_intervals_micros,
            median_nonzero(&native.completed_write_intervals_micros),
            &mut defects,
        );
    } else {
        defects.push(TimingDefect {
            reason: "missing_candidate_native_timing".to_owned(),
            expected: "Harness receive-to-flush metrics".to_owned(),
            observed: "missing".to_owned(),
        });
    }
    if defects.is_empty() {
        Ok(())
    } else {
        let defects_len = defects.len();
        Err(ComparatorError::Timing {
            defects,
            defects_len,
        })
    }
}

fn p95(values: &[u64]) -> Option<u64> {
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = (values.len().saturating_mul(95).saturating_add(99)) / 100;
    values.get(rank.saturating_sub(1)).copied()
}

fn check_intervals(side: &str, intervals: &[u64], cadence: u64, defects: &mut Vec<TimingDefect>) {
    if cadence == 0 {
        return;
    }
    if let Some(gap) = intervals
        .iter()
        .copied()
        .find(|gap| *gap > cadence.saturating_mul(MAX_GAP_MULTIPLIER))
    {
        defects.push(TimingDefect {
            reason: "maximum_gap".to_owned(),
            expected: format!("{side} <= {}us", cadence.saturating_mul(2)),
            observed: format!("{gap}us"),
        });
    }
}
