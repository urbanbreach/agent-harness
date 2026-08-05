use super::error::ComparatorError;

pub const P95_SMOOTHNESS_PERCENT: u64 = 125;
pub const MAX_CADENCE_GAP_MULTIPLIER: u64 = 2;
pub const MAX_PHASE_WINDOW_MS: u64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimingPhase {
    Rest,
    Mid,
    Settled,
}

const REQUIRED_PHASE_ORDER: [TimingPhase; 3] =
    [TimingPhase::Rest, TimingPhase::Mid, TimingPhase::Settled];

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
    pub phase_order: Vec<TimingPhase>,
}

impl TimingTrace {
    pub fn new(frame_timestamps_ms: Vec<u64>, latency_ms: Vec<u64>) -> Self {
        let phase_order = if frame_timestamps_ms.len() == REQUIRED_PHASE_ORDER.len() {
            REQUIRED_PHASE_ORDER.to_vec()
        } else {
            Vec::new()
        };
        Self::with_phase_order(frame_timestamps_ms, latency_ms, phase_order)
    }

    pub fn with_phase_order(
        frame_timestamps_ms: Vec<u64>,
        latency_ms: Vec<u64>,
        phase_order: Vec<TimingPhase>,
    ) -> Self {
        Self {
            frame_timestamps_ms,
            latency_ms: LatencyDistribution::new(latency_ms),
            phase_order,
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
        let reference_start = reference.frame_timestamps_ms.first().copied();
        let candidate_start = candidate.frame_timestamps_ms.first().copied();
        for (index, (left, right)) in reference
            .frame_timestamps_ms
            .iter()
            .zip(&candidate.frame_timestamps_ms)
            .enumerate()
        {
            let drift = match (reference_start, candidate_start) {
                (Some(reference_start), Some(candidate_start)) => left
                    .saturating_sub(reference_start)
                    .abs_diff(right.saturating_sub(candidate_start)),
                _ => 0,
            };
            if drift > super::motion::FRAME_TIMESTAMP_TOLERANCE_MS {
                defects.push(TimingDefect {
                    reason: "timestamp_drift".to_owned(),
                    expected: format!("frame {index} within 16ms of {left}"),
                    observed: right.to_string(),
                });
            }
        }
    }
    compare_p95(reference, candidate, &mut defects);
    check_phase_order("reference", reference, &mut defects);
    check_phase_order("candidate", candidate, &mut defects);
    check_phase_windows("reference", reference, &mut defects);
    check_phase_windows("candidate", candidate, &mut defects);
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

fn check_phase_order(_side: &str, trace: &TimingTrace, defects: &mut Vec<TimingDefect>) {
    if trace.phase_order != REQUIRED_PHASE_ORDER {
        defects.push(TimingDefect {
            reason: "phase_order".to_owned(),
            expected: "rest -> mid -> settled".to_owned(),
            observed: format!("{} phases", trace.phase_order.len()),
        });
    }
    if trace.frame_timestamps_ms.len() != trace.phase_order.len() {
        defects.push(TimingDefect {
            reason: "phase_count".to_owned(),
            expected: trace.phase_order.len().to_string(),
            observed: trace.frame_timestamps_ms.len().to_string(),
        });
    }
}

fn check_phase_windows(side: &str, trace: &TimingTrace, defects: &mut Vec<TimingDefect>) {
    let Some(expected_gap) = trace
        .frame_timestamps_ms
        .windows(2)
        .next()
        .map(|pair| pair[1].saturating_sub(pair[0]))
    else {
        return;
    };
    for (index, pair) in trace.frame_timestamps_ms.windows(2).enumerate() {
        let gap = pair[1].saturating_sub(pair[0]);
        if gap > MAX_PHASE_WINDOW_MS
            || gap > expected_gap.saturating_mul(MAX_CADENCE_GAP_MULTIPLIER)
        {
            defects.push(TimingDefect {
                reason: "phase_window".to_owned(),
                expected: format!(
                    "{side} phase gap <= {}ms and <= {MAX_PHASE_WINDOW_MS}ms at {index}",
                    expected_gap.saturating_mul(MAX_CADENCE_GAP_MULTIPLIER)
                ),
                observed: format!("{gap}ms"),
            });
        }
    }
}
