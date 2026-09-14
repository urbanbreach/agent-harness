//! Senpi's adaptive compaction policy, expressed in integer percentages.
use crate::event::SessionCompactionEvent;

pub(super) fn threshold_percent(window: u32, previous: Option<&SessionCompactionEvent>) -> u32 {
    let base: u32 = match window {
        0 => 50,
        1..=16_000 => 45,
        16_001..=32_000 => 50,
        32_001..=64_000 => 55,
        64_001..=128_000 => 60,
        128_001..=512_000 => 70,
        _ => 80,
    };
    let Some(previous) = previous.filter(|previous| previous.tokens_before > 0) else {
        return base;
    };
    let saved = previous
        .tokens_before
        .saturating_sub(previous.tokens_after.unwrap_or(previous.tokens_before));
    if u64::from(saved) * 2 > u64::from(previous.tokens_before) {
        base.saturating_sub(5).clamp(40, 85)
    } else {
        base
    }
}

pub(super) fn threshold_tokens(window: u32, previous: Option<&SessionCompactionEvent>) -> u32 {
    u32::try_from(u64::from(window) * u64::from(threshold_percent(window, previous)) / 100)
        .unwrap_or(u32::MAX)
}

pub(super) fn keep_recent_tokens(
    setting: u32,
    window: u32,
    previous: Option<&SessionCompactionEvent>,
) -> u32 {
    let scaled = if window > 409_600 && setting >= 10_000 {
        setting.max((window / 20).min(60_000))
    } else {
        setting
    };
    let cap = u64::from(window) * u64::from(95 - threshold_percent(window, previous)) / 100;
    scaled.min(u32::try_from(cap).unwrap_or(u32::MAX).max(1024))
}

pub(super) fn reserve_tokens(setting: u32, window: u32) -> u32 {
    setting.max((window / 25).min(49_152))
}

#[derive(Default)]
pub(in crate::coord) struct CompactionState {
    count: u32,
    pub(in crate::coord) last_started: Option<u64>,
    pub(in crate::coord) warm: Option<super::GeneratedSessionCompaction>,
    tripped_at: Option<u64>,
}

impl CompactionState {
    pub(in crate::coord) fn tripped(&self, now: u64) -> bool {
        self.tripped_at
            .is_some_and(|start| now < start.saturating_add(60_000))
    }

    pub(in crate::coord) fn record(&mut self, success: bool, now: u64) {
        if success {
            self.count = 0;
            self.tripped_at = None;
            return;
        }
        if self.tripped_at.is_some() && !self.tripped(now) {
            self.count = 0;
            self.tripped_at = None;
        }
        self.count = self.count.saturating_add(1);
        if self.count >= 3 {
            self.tripped_at.get_or_insert(now);
        }
    }
}

pub(super) fn lead_tokens(threshold: u32) -> u32 {
    (threshold / 8).clamp(8192, 32_768)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn senpi_compaction_policy_scales_windows_and_preserves_grace_headroom() {
        for (window, threshold, recent, reserve) in [
            (16_000, 7_200, 8_000, 16_384),
            (32_000, 16_000, 14_400, 16_384),
            (64_000, 35_200, 20_000, 16_384),
            (128_000, 76_800, 20_000, 16_384),
            (512_000, 358_400, 25_600, 20_480),
            (1_000_000, 800_000, 50_000, 40_000),
        ] {
            assert_eq!(threshold_tokens(window, None), threshold);
            assert_eq!(keep_recent_tokens(20_000, window, None), recent);
            assert_eq!(reserve_tokens(16_384, window), reserve);
            assert!((8192..=32_768).contains(&lead_tokens(threshold)));
        }
    }
}
