use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FramePhase {
    Input,
    Render,
    Flush,
    Settled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMetrics {
    pub input_to_render_us: u64,
    pub render_to_flush_us: u64,
    pub total_frame_us: u64,
    pub phase: FramePhase,
}

impl FrameMetrics {
    pub fn meets_target_fps(self, target_fps: u16) -> bool {
        target_fps != 0 && self.total_frame_us <= 1_000_000 / u64::from(target_fps)
    }

    pub fn within_110pct_gate(self, reference_us: u64) -> bool {
        self.total_frame_us <= reference_us.saturating_mul(110) / 100
    }

    pub fn cadence_gap_ok(self, prev: Self) -> bool {
        cadence_ratio(self.total_frame_us, prev.total_frame_us) <= 200
    }
}

pub struct FrameClock {
    frames: VecDeque<FrameMetrics>,
    capacity: usize,
    last_total_us: u64,
}

impl FrameClock {
    pub fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
            last_total_us: 0,
        }
    }

    pub fn record(&mut self, metrics: FrameMetrics) {
        self.frames.push_back(metrics);
        if self.frames.len() > self.capacity {
            self.frames.pop_front();
        }
        self.last_total_us = metrics.total_frame_us;
    }

    pub fn p95_total_us(&self) -> u64 {
        let mut totals: Vec<u64> = self
            .frames
            .iter()
            .map(|frame| frame.total_frame_us)
            .collect();
        if totals.is_empty() {
            return 0;
        }
        totals.sort_unstable();
        totals[(totals.len() * 95 / 100).min(totals.len() - 1)]
    }

    pub fn max_cadence_gap(&self) -> u64 {
        self.frames
            .iter()
            .zip(self.frames.iter().skip(1))
            .map(|(first, second)| cadence_ratio(first.total_frame_us, second.total_frame_us))
            .max()
            .unwrap_or(0)
    }

    pub fn settled_redraw_count(&self) -> usize {
        self.frames
            .iter()
            .filter(|frame| frame.phase == FramePhase::Settled)
            .count()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new(300)
    }
}

fn cadence_ratio(first: u64, second: u64) -> u64 {
    if first == 0 || second == 0 {
        return 0;
    }
    let larger = first.max(second);
    let smaller = first.min(second);
    larger.saturating_mul(100) / smaller
}
