use std::collections::VecDeque;

use crate::perf_budgets::frame::{FrameMetrics, FramePhase};
use crate::perf_budgets::resources::ResourceSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StressSample {
    pub tick: u64,
    pub frame_metrics: FrameMetrics,
    pub resources: ResourceSnapshot,
}

pub struct SampleWindow {
    samples: VecDeque<StressSample>,
    capacity: usize,
}

impl SampleWindow {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }
    pub fn record(&mut self, sample: StressSample) {
        self.samples.push_back(sample);
        if self.samples.len() > self.capacity {
            self.samples.pop_front();
        }
    }
    pub fn p95_frame_budget_gate(&self, baseline_us: u64) -> bool {
        let mut totals: Vec<u64> = self
            .samples
            .iter()
            .map(|sample| sample.frame_metrics.total_frame_us)
            .collect();
        if totals.is_empty() {
            return true;
        }
        totals.sort_unstable();
        totals[(totals.len() * 95 / 100).min(totals.len() - 1)]
            <= baseline_us.saturating_mul(110) / 100
    }
    pub fn max_cadence_gap_ratio(&self) -> u64 {
        self.samples
            .iter()
            .zip(self.samples.iter().skip(1))
            .map(|(first, second)| {
                cadence_ratio(
                    first.frame_metrics.total_frame_us,
                    second.frame_metrics.total_frame_us,
                )
            })
            .max()
            .unwrap_or(0)
    }
    pub fn settled_redraw_count(&self) -> usize {
        self.samples
            .iter()
            .filter(|sample| sample.frame_metrics.phase == FramePhase::Settled)
            .count()
    }
    pub fn memory_growth_over_window(&self) -> bool {
        self.samples.back().is_some_and(|last| {
            let resources: Vec<ResourceSnapshot> =
                self.samples.iter().map(|sample| sample.resources).collect();
            last.resources.has_sustained_growth(&resources)
        })
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

impl Default for SampleWindow {
    fn default() -> Self {
        Self::new(9000)
    }
}

fn cadence_ratio(first: u64, second: u64) -> u64 {
    if first == 0 || second == 0 {
        0
    } else {
        first.max(second).saturating_mul(100) / first.min(second)
    }
}
