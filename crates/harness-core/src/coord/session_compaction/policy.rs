//! Senpi's adaptive compaction policy, expressed in integer percentages.
use crate::conversation::ConversationMessage;
use crate::event::{EventEnvelopeV1, EventV1};

pub(super) fn threshold_percent(window: u32, previous: Option<(u32, u32)>) -> u32 {
    let base: u32 = match window {
        0 => 50,
        1..=16_000 => 45,
        16_001..=32_000 => 50,
        32_001..=64_000 => 55,
        64_001..=128_000 => 60,
        128_001..=512_000 => 70,
        _ => 80,
    };
    let Some((saved, before)) = previous.filter(|(_, before)| *before > 0) else {
        return base;
    };
    if u64::from(saved) * 2 > u64::from(before) {
        base.saturating_sub(5).clamp(40, 85)
    } else {
        base
    }
}

/// Derive structural savings from the journal, including after resume. Provider
/// overhead and retained messages are not savings from replacing old history.
pub(super) fn previous_yield(events: &[EventEnvelopeV1], agent_id: &str) -> Option<(u32, u32)> {
    let mut compactions = events
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(index, event)| match &event.payload {
            EventV1::SessionCompaction(data) if data.agent_id == agent_id => Some((index, data)),
            _ => None,
        });
    let (index, latest) = compactions.next()?;
    let previous = compactions.next().map(|(_, data)| data);
    let first_seq = previous.map_or(0, |data| data.first_kept_event_seq);
    let old_summary_tokens = previous.map_or(0, |data| {
        super::super::compaction::estimate_text_tokens(&data.summary)
    });
    let replaced =
        super::preparation::build_agent_conversation_messages(&events[..index], agent_id)
            .iter()
            .filter(|message| !matches!(message, ConversationMessage::Checkpoint(_)))
            .filter(|message| {
                (first_seq..latest.first_kept_event_seq)
                    .contains(&super::preparation::message_seq(message))
            })
            .map(super::super::compaction::estimate_message_tokens)
            .fold(old_summary_tokens, u32::saturating_add);
    Some((
        replaced.saturating_sub(super::super::compaction::estimate_text_tokens(
            &latest.summary,
        )),
        latest.tokens_before,
    ))
}

pub(super) fn threshold_tokens(threshold_hundredths: u64) -> u32 {
    // Integer usage must reach the fractional threshold, matching upstream's >= comparison.
    u32::try_from(threshold_hundredths.div_ceil(100)).unwrap_or(u32::MAX)
}

pub(super) fn keep_recent_tokens(setting: u32, window: u32, threshold_hundredths: u64) -> u32 {
    let scaled = if window > 409_600 && setting >= 10_000 {
        setting.max((window / 20).min(60_000))
    } else {
        setting
    };
    let cap = (u64::from(window) * 95).saturating_sub(threshold_hundredths) / 100;
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

pub(super) fn lead_tokens(threshold_hundredths: u64) -> u32 {
    u32::try_from(threshold_hundredths / 800)
        .unwrap_or(u32::MAX)
        .clamp(8192, 32_768)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn senpi_compaction_policy_scales_windows_and_preserves_grace_headroom() {
        // Expected values executed from inspirations/senpi/.../compaction/policy.ts:
        // ceil(window * computeEffectiveThreshold), computeEffectiveKeepRecentTokens,
        // and resolveReserveTokens. Include both sides of every policy boundary.
        for (window, threshold, recent, reserve) in [
            (0, 0, 1024, 16384),
            (1, 1, 1024, 16384),
            (15999, 7200, 7999, 16384),
            (16000, 7200, 8000, 16384),
            (16001, 8001, 7200, 16384),
            (31999, 16000, 14399, 16384),
            (32000, 16000, 14400, 16384),
            (32001, 17601, 12800, 16384),
            (63999, 35200, 20000, 16384),
            (64000, 35200, 20000, 16384),
            (64001, 38401, 20000, 16384),
            (127999, 76800, 20000, 16384),
            (128000, 76800, 20000, 16384),
            (128001, 89601, 20000, 16384),
            (409600, 286720, 20000, 16384),
            (409601, 286721, 20480, 16384),
            (511999, 358400, 25599, 20479),
            (512000, 358400, 25600, 20480),
            (512001, 409601, 25600, 20480),
            (1000000, 800000, 50000, 40000),
            (4294967295, 3435973836, 60000, 49152),
        ] {
            let percent = threshold_percent(window, None);
            let threshold_hundredths = u64::from(window) * u64::from(percent);
            assert_eq!(
                threshold_tokens(threshold_hundredths),
                threshold,
                "window={window}"
            );
            assert_eq!(
                keep_recent_tokens(20_000, window, threshold_hundredths),
                recent,
                "window={window}"
            );
            assert_eq!(reserve_tokens(16_384, window), reserve);
            assert!((8192..=32_768).contains(&lead_tokens(threshold_hundredths)));
        }
    }

    #[test]
    fn compaction_yield_and_speculation_lead_respect_exact_boundaries() {
        for (saved, expected) in [(0, 45), (1000, 45), (5000, 45), (5001, 40), (10000, 40)] {
            assert_eq!(threshold_percent(16000, Some((saved, 10000))), expected);
        }
        assert_eq!(threshold_percent(16000, Some((5001, 0))), 45);
        for (threshold, lead) in [
            (0, 8192),
            (65536, 8192),
            (65544, 8193),
            (262144, 32768),
            (262152, 32768),
        ] {
            assert_eq!(lead_tokens(threshold * 100), lead);
        }
        assert_eq!(lead_tokens(93_633 * 70), 8192);
        assert_eq!(lead_tokens(93_635 * 70), 8193);
        assert_eq!(keep_recent_tokens(20_000, 32_000, 12_345 * 100), 18_055);
        assert_eq!(
            keep_recent_tokens(20_000, 32_000, u64::from(u32::MAX) * 100),
            1024
        );
    }

    #[test]
    fn compaction_breaker_trips_after_three_failures_and_resets_at_cooldown() {
        let mut state = CompactionState::default();
        for now in 0..3 {
            state.record(false, now);
            assert_eq!(state.tripped(now), now == 2);
        }
        assert!(state.tripped(60_001));
        assert!(!state.tripped(60_002));
        state.record(false, 60_002);
        assert!(!state.tripped(60_002));
        state.record(false, 60_003);
        state.record(true, 60_004);
        state.record(false, 60_005);
        assert!(!state.tripped(60_005));
    }
}
