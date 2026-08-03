use super::error::ComparatorError;
use super::motion::CADENCE_MAX_GAP_MS;

pub const P95_SMOOTHNESS_PERCENT: u64 = 110;
pub const MAX_CADENCE_GAP_MULTIPLIER: u64 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LatencyDistribution {
    pub samples_ms: Vec<u64>,
}

impl LatencyDistribution {
    pub fn new(samples_ms: Vec<u64>) -> Self {
        Self { samples_ms }
    }

    pub fn p95(&self) -> Option<u64> {
        if self.samples_ms.is_empty() {
            return None;
        }
        let mut sorted = self.samples_ms.clone();
        sorted.sort_unstable();
        let rank = (sorted.len() * 95).saturating_add(99) / 100;
        sorted.get(rank.saturating_sub(1)).copied()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingTrace {
    pub frame_timestamps_ms: Vec<u64>,
    pub latency_ms: LatencyDistribution,
}

impl TimingTrace {
    pub fn new(frame_timestamps_ms: Vec<u64>, latency_ms: Vec<u64>) -> Self {
        Self {
            frame_timestamps_ms,
            latency_ms: LatencyDistribution::new(latency_ms),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingDefect {
    pub reason: String,
    pub expected: String,
    pub observed: String,
}

pub fn compare_timing(
    reference: &TimingTrace,
    candidate: &TimingTrace,
) -> Result<(), ComparatorError> {
    let mut defects = Vec::new();
    if reference.frame_timestamps_ms.len() != candidate.frame_timestamps_ms.len() {
        defects.push(TimingDefect {
            reason: "frame_count".to_owned(),
            expected: reference.frame_timestamps_ms.len().to_string(),
            observed: candidate.frame_timestamps_ms.len().to_string(),
        });
    } else {
        for (index, (left, right)) in reference
            .frame_timestamps_ms
            .iter()
            .zip(&candidate.frame_timestamps_ms)
            .enumerate()
        {
            if left.abs_diff(*right) > super::motion::FRAME_TIMESTAMP_TOLERANCE_MS {
                defects.push(TimingDefect {
                    reason: "timestamp_drift".to_owned(),
                    expected: format!("frame {index} within 16ms of {left}"),
                    observed: right.to_string(),
                });
            }
        }
    }
    compare_p95(reference, candidate, &mut defects);
    check_cadence("reference", &reference.frame_timestamps_ms, &mut defects);
    check_cadence("candidate", &candidate.frame_timestamps_ms, &mut defects);
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

fn compare_p95(reference: &TimingTrace, candidate: &TimingTrace, defects: &mut Vec<TimingDefect>) {
    let (Some(reference_p95), Some(candidate_p95)) =
        (reference.latency_ms.p95(), candidate.latency_ms.p95())
    else {
        defects.push(TimingDefect {
            reason: "empty_latency_distribution".to_owned(),
            expected: "non-empty p95".to_owned(),
            observed: "empty".to_owned(),
        });
        return;
    };
    if candidate_p95.saturating_mul(100) > reference_p95.saturating_mul(P95_SMOOTHNESS_PERCENT) {
        defects.push(TimingDefect {
            reason: "p95_smoothness".to_owned(),
            expected: format!("<= {}% of {reference_p95}ms", P95_SMOOTHNESS_PERCENT),
            observed: format!("{candidate_p95}ms"),
        });
    }
}

fn check_cadence(side: &str, timestamps: &[u64], defects: &mut Vec<TimingDefect>) {
    for (index, pair) in timestamps.windows(2).enumerate() {
        let gap = pair[1].saturating_sub(pair[0]);
        if gap > CADENCE_MAX_GAP_MS {
            defects.push(TimingDefect {
                reason: "cadence_gap".to_owned(),
                expected: format!("{side} gap <= {CADENCE_MAX_GAP_MS}ms at {index}"),
                observed: format!("{gap}ms"),
            });
        }
    }
}
