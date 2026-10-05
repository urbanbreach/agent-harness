use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

pub(super) const FRAME_INTERVAL: Duration = Duration::from_millis(16);

pub(super) struct StreamingText {
    text: String,
    visible: usize,
    last_grapheme: usize,
    ready_at: Instant,
    tick_at: Instant,
    arrival_at: Instant,
    rate: u64,
    carry: u64,
}

impl StreamingText {
    pub(super) fn new(text: &str, visible: usize, now: Instant) -> Self {
        let mut stream = Self {
            text: text.to_owned(),
            visible,
            last_grapheme: 0,
            ready_at: now + Duration::from_millis(80),
            tick_at: now,
            arrival_at: now,
            rate: 90,
            carry: 0,
        };
        stream.align_visible();
        stream
    }

    pub(super) fn update(&mut self, text: &str, now: Instant) {
        if !text.starts_with(&self.text) {
            // A replaced or truncated snapshot is already current, not another delta.
            *self = Self::new(text, text.len(), now);
            return;
        }
        let appended = &text[self.text.len()..];
        if appended.is_empty() {
            return;
        }
        if !self.pending() {
            self.tick_at = now;
            self.carry = 0;
        }
        let elapsed = millis(now.saturating_duration_since(self.arrival_at));
        let units = u64::try_from(appended.graphemes(true).count()).unwrap_or(u64::MAX);
        if let Some(sample) = units.saturating_mul(1000).checked_div(elapsed) {
            self.rate = (self.rate * 3 + sample.clamp(45, self.rate * 4)) / 4;
        }
        self.arrival_at = now;
        self.text.push_str(appended);
        // A later chunk can extend an already visible combining or ZWJ cluster.
        self.align_visible();
    }

    fn align_visible(&mut self) {
        for (start, grapheme) in self.text[self.last_grapheme..].grapheme_indices(true) {
            let start = self.last_grapheme + start;
            if start + grapheme.len() >= self.visible {
                if self.visible > start {
                    self.visible = start + grapheme.len();
                    self.last_grapheme = start;
                }
                break;
            }
        }
    }

    pub(super) fn pending(&self) -> bool {
        self.visible < self.text.len()
    }

    pub(super) fn visible(&self) -> &str {
        &self.text[..self.visible]
    }

    pub(super) fn finish(&mut self) {
        self.visible = self.text.len();
    }

    pub(super) fn advance(&mut self, now: Instant) -> bool {
        if !self.pending() || now < self.ready_at || now < self.tick_at + FRAME_INTERVAL {
            return false;
        }
        let elapsed =
            millis(now.saturating_duration_since(self.tick_at.max(self.ready_at))).min(100);
        self.tick_at = now;
        let tail = &self.text[self.visible..];
        // Only the catch-up threshold matters; scanning huge output every frame does not.
        let target = self.rate * 140 / 1000;
        let limit = usize::try_from(target.saturating_add(161)).unwrap_or(usize::MAX);
        let backlog = u64::try_from(tail.graphemes(true).take(limit).count()).unwrap_or(u64::MAX);
        let rate = if backlog >= target {
            self.rate + ((backlog - target) * 1000 / 267).min(600)
        } else {
            self.rate.saturating_sub((target - backlog) * 1000 / 267)
        };
        self.carry += rate * elapsed;
        let step = usize::try_from(self.carry / 1000).unwrap_or(usize::MAX);
        self.carry %= 1000;
        let previous = self.visible;
        for (start, grapheme) in tail.grapheme_indices(true).take(step) {
            self.last_grapheme = previous + start;
            self.visible = self.last_grapheme + grapheme.len();
        }
        self.visible != previous
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveal_drains_without_new_chunks_and_never_splits_a_grapheme() {
        let now = Instant::now();
        let text = "界e\u{301}👩🏽‍💻";
        let mut stream = StreamingText::new(text, 0, now);
        assert!(!stream.advance(now + Duration::from_millis(64)));
        for frame in 5..80 {
            stream.advance(now + Duration::from_millis(frame * 16));
            assert!(["", "界", "界e\u{301}", text].contains(&stream.visible()));
        }
        assert_eq!(stream.visible(), text);
        assert!(!stream.pending());

        // The next transport chunk extends the already painted final grapheme.
        let mut stream = StreamingText::new("e", 1, now);
        stream.update("e\u{301}👩", now + Duration::from_millis(16));
        assert_eq!(stream.visible(), "e\u{301}");
        stream.advance(now + Duration::from_secs(1));
        stream.update("e\u{301}👩🏽‍💻more", now + Duration::from_millis(1016));
        assert_eq!(stream.visible(), "e\u{301}👩🏽‍💻");
        stream.update("replacement", now + Duration::from_secs(2));
        assert_eq!(stream.visible(), "replacement");
        assert!(!stream.pending());
    }
}
