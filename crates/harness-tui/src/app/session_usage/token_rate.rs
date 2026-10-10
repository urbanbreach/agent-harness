//! Live generation throughput, after oh-my-pi's `TokenRateMeter`.
//!
//! The reading is a kernel-weighted rate over recent stream time: token and time sums decayed
//! on three half-lives, so a burst shifts it in proportion to its token mass instead of
//! replacing it. Streamed deltas count at about four UTF-8 bytes per token while a request is
//! open. Tokens the provider bills but never streams (hidden reasoning, tool-call framing,
//! estimate error) form a hidden-token rate per millisecond of request span: the open request
//! is credited at that rate, and its billed count settles the difference when it lands.
//! Stream time runs only while a request is open, so tool execution between requests holds
//! the reading.
use std::f64::consts::LN_2;

const HALF_LIVES_MS: [f64; 3] = [5_000.0, 20_000.0, 80_000.0];
/// Evidence gate on the longest scale: a provider's first buffered chunk can carry hundreds
/// of tokens in one delta, which is not a rate yet.
const MIN_TOKENS: f64 = 200.0;
const MIN_TIME_MS: f64 = 4_000.0;
/// Streamed bytes are credited to the decayed sums once per bucket.
const BUCKET_MS: u64 = 250;
const BYTES_PER_TOKEN: f64 = 4.0;
/// Weight kept from earlier requests when a new billed count refines the hidden-token rate.
const HIDDEN_DECAY: f64 = 0.8;
/// Prior pulling the hidden-token rate toward zero, weighted like a request of this span.
const HIDDEN_PRIOR_MS: f64 = 10_000.0;

/// Decayed token and time sums on one half-life. `time` is the exact integral of the kernel
/// over stream time, so `tokens / time` is a kernel-weighted rate.
#[derive(Clone, Copy, Default)]
struct Sums {
    tokens: f64,
    time: f64,
}

impl Sums {
    /// Ages both sums by `dt_ms`; an open request also accrues the kernel integral as time
    /// and `hidden_rate` tokens per millisecond of it.
    fn advance(&mut self, dt_ms: f64, half_life_ms: f64, hidden_rate: Option<f64>) {
        let decay = (-dt_ms / half_life_ms).exp2();
        self.tokens *= decay;
        self.time *= decay;
        if let Some(rate) = hidden_rate {
            let integral = half_life_ms / LN_2 * (1.0 - decay);
            self.time += integral;
            self.tokens += rate * integral;
        }
    }
}

#[derive(Default)]
pub(super) struct TokenRateMeter {
    history: [Sums; 3],
    inflight: [Sums; 3],
    /// Stream time the open request started, or `None` between requests.
    started_ms: Option<u64>,
    advanced_ms: u64,
    /// Streamed tokens of the open request already credited to `inflight`.
    inflight_local: f64,
    /// Hidden-token rate (tokens/ms) credited to the open request; fixed at `begin`.
    inflight_hidden_rate: f64,
    bucket: u64,
    pending_bytes: u64,
    hidden_tokens: f64,
    hidden_span_ms: f64,
}

impl TokenRateMeter {
    /// Opens a request at stream time `now_ms`; a request still open is dropped unbilled.
    pub(super) fn begin(&mut self, now_ms: u64) {
        self.clear_inflight();
        self.started_ms = Some(now_ms);
        self.advanced_ms = now_ms;
        self.inflight_hidden_rate =
            (self.hidden_tokens / (self.hidden_span_ms + HIDDEN_PRIOR_MS)).max(0.0);
    }

    /// Records `bytes` of streamed text, reasoning or tool input of the open request.
    pub(super) fn push(&mut self, bytes: usize, now_ms: u64) {
        let Some(started_ms) = self.started_ms.filter(|_| bytes > 0) else {
            return;
        };
        let bucket = now_ms.saturating_sub(started_ms) / BUCKET_MS;
        if bucket != self.bucket {
            self.flush(now_ms);
            self.bucket = bucket;
        }
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        self.pending_bytes = self.pending_bytes.saturating_add(bytes);
    }

    /// Closes the open request. Without a billed count (an aborted stream) the streamed
    /// estimate stands and the hidden-token rate is left alone.
    pub(super) fn end(&mut self, billed: Option<u32>, now_ms: u64) {
        let Some(started_ms) = self.started_ms else {
            return;
        };
        self.flush(now_ms);
        self.advance(now_ms);
        let span_ms = as_f64(now_ms.saturating_sub(started_ms));
        let mut extra = 0.0;
        if let Some(billed) = billed.filter(|billed| *billed > 0) {
            let hidden = f64::from(billed) - self.inflight_local;
            self.hidden_tokens = self.hidden_tokens * HIDDEN_DECAY + hidden;
            self.hidden_span_ms = self.hidden_span_ms * HIDDEN_DECAY + span_ms;
            extra = hidden - self.inflight_hidden_rate * span_ms;
        }
        for (history, live) in self.history.iter_mut().zip(&self.inflight) {
            // `extra` spread uniformly over the span has kernel weight `time / span`.
            let spread = if span_ms > 0.0 {
                extra * live.time / span_ms
            } else {
                0.0
            };
            history.tokens += (live.tokens + spread).max(0.0);
            history.time += live.time;
        }
        self.clear_inflight();
    }

    /// Blanks the reading until a new run accumulates enough evidence.
    pub(super) fn reset(&mut self) {
        self.clear_inflight();
        self.history = [Sums::default(); 3];
    }

    /// Restores the reading from a completed request, scaled past the evidence gate so short
    /// requests read immediately. It holds until new requests blend with it.
    pub(super) fn seed(&mut self, output_tokens: u32, duration_ms: u64) {
        if output_tokens == 0 || duration_ms == 0 {
            self.reset();
            return;
        }
        self.clear_inflight();
        let (tokens, time) = (f64::from(output_tokens), as_f64(duration_ms));
        let scale = (MIN_TOKENS / tokens).max(MIN_TIME_MS / time).max(1.0);
        self.history = [Sums {
            tokens: tokens * scale,
            time: time * scale,
        }; 3];
    }

    /// Tokens per second over the decayed window, or `None` until enough evidence.
    pub(super) fn rate(&self, now_ms: u64) -> Option<f64> {
        let dt_ms = self
            .started_ms
            .map_or(0.0, |_| as_f64(now_ms.saturating_sub(self.advanced_ms)));
        let pending = as_f64(self.pending_bytes) / BYTES_PER_TOKEN;
        let (mut tokens, mut time, mut evidence) = (0.0, 0.0, (0.0, 0.0));
        for ((history, live), half_life_ms) in
            self.history.iter().zip(&self.inflight).zip(HALF_LIVES_MS)
        {
            let decay = (-dt_ms / half_life_ms).exp2();
            let integral = half_life_ms / LN_2 * (1.0 - decay);
            evidence = (
                (history.tokens + live.tokens) * decay
                    + self.inflight_hidden_rate * integral
                    + pending,
                (history.time + live.time) * decay + integral,
            );
            tokens += evidence.0;
            time += evidence.1;
        }
        (evidence.0 >= MIN_TOKENS && evidence.1 >= MIN_TIME_MS).then(|| tokens * 1000.0 / time)
    }

    /// Ages every sum to `now_ms`; stream time only advances inside a request.
    fn advance(&mut self, now_ms: u64) {
        let dt_ms = as_f64(now_ms.saturating_sub(self.advanced_ms));
        if dt_ms <= 0.0 {
            return;
        }
        self.advanced_ms = now_ms;
        for ((history, live), half_life_ms) in self
            .history
            .iter_mut()
            .zip(&mut self.inflight)
            .zip(HALF_LIVES_MS)
        {
            history.advance(dt_ms, half_life_ms, None);
            live.advance(dt_ms, half_life_ms, Some(self.inflight_hidden_rate));
        }
    }

    /// Credits the pending bucket to the open request at `now_ms`.
    fn flush(&mut self, now_ms: u64) {
        if self.pending_bytes == 0 {
            return;
        }
        self.advance(now_ms);
        let tokens = as_f64(std::mem::take(&mut self.pending_bytes)) / BYTES_PER_TOKEN;
        for live in &mut self.inflight {
            live.tokens += tokens;
        }
        self.inflight_local += tokens;
    }

    fn clear_inflight(&mut self) {
        self.inflight = [Sums::default(); 3];
        self.started_ms = None;
        self.inflight_local = 0.0;
        self.inflight_hidden_rate = 0.0;
        self.bucket = 0;
        self.pending_bytes = 0;
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "stream milliseconds and byte counts stay far below 2^52"
)]
const fn as_f64(value: u64) -> f64 {
    value as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: usize = 4;

    /// Streams `per_delta` tokens every 100 ms over `[from, to)`.
    fn stream(meter: &mut TokenRateMeter, from: u64, to: u64, per_delta: usize) {
        for at in (from..to).step_by(100) {
            meter.push(per_delta * TOKEN, at);
        }
    }

    /// `count` requests of 600 tokens over 10 s (60 tok/s) with 5 s tool gaps.
    fn settle(meter: &mut TokenRateMeter, count: u64) -> u64 {
        let mut clock = 0;
        for _ in 0..count {
            meter.begin(clock);
            stream(meter, clock, clock + 10_000, 6);
            clock += 10_000;
            meter.end(Some(600), clock);
            clock += 5_000;
        }
        clock
    }

    fn reading(meter: &TokenRateMeter, now_ms: u64) -> f64 {
        meter.rate(now_ms).unwrap_or(0.0)
    }

    #[test]
    fn rate_waits_for_enough_tokens_and_stream_time() {
        let mut meter = TokenRateMeter::default();
        meter.begin(0);
        // A provider's first buffered chunk: 300 tokens in one delta.
        meter.push(300 * TOKEN, 500);
        assert_eq!(meter.rate(600), None);
        stream(&mut meter, 600, 10_000, 6);
        let rate = reading(&meter, 10_000);
        assert!((80.0..100.0).contains(&rate), "{rate}");
    }

    #[test]
    fn rate_holds_across_tool_execution_and_a_short_burst_only_nudges_it() {
        let mut meter = TokenRateMeter::default();
        let clock = settle(&mut meter, 6);
        assert!((reading(&meter, clock) - 60.0).abs() < 0.5);
        assert!((reading(&meter, clock + 60_000) - 60.0).abs() < 0.5);
        // A 2 s burst of tool arguments at 200 tok/s.
        meter.begin(clock);
        stream(&mut meter, clock, clock + 2_000, 20);
        let peak = reading(&meter, clock + 2_000);
        assert!((60.0..80.0).contains(&peak), "{peak}");
    }

    #[test]
    fn billed_tokens_never_streamed_are_charged_to_their_span_without_amplifying_bursts() {
        // 20 s of hidden reasoning, then a 2-token tool call; the provider bills 1500.
        let mut meter = TokenRateMeter::default();
        meter.begin(0);
        meter.push(2 * TOKEN, 20_000);
        meter.end(Some(1_500), 20_100);
        assert!((reading(&meter, 20_100) - 1_500.0 / 20.1).abs() < 0.5);

        // Ten 25-token tool calls over 2 s each, billed 80: a ~55-token envelope, ~40 tok/s.
        let mut meter = TokenRateMeter::default();
        let mut clock = 0;
        for _ in 0..10 {
            meter.begin(clock);
            meter.push(25 * TOKEN, clock + 1_500);
            clock += 2_000;
            meter.end(Some(80), clock);
            clock += 3_000;
        }
        assert!((reading(&meter, clock) - 40.0).abs() < 5.0);
        // 2000 visible tokens over 20 s at a true 100 tok/s. Scaling by the envelope
        // (80/25) would read ~300; adding it stays honest.
        meter.begin(clock);
        let mut peak: f64 = 0.0;
        for at in (0..20_000).step_by(100) {
            meter.push(10 * TOKEN, clock + at);
            peak = peak.max(reading(&meter, clock + at));
        }
        assert!(peak < 115.0, "{peak}");
        meter.end(Some(2_055), clock + 20_000);
        let settled = reading(&meter, clock + 20_000);
        assert!((80.0..105.0).contains(&settled), "{settled}");
    }

    #[test]
    fn seed_reads_a_short_request_immediately_and_holds() {
        let mut meter = TokenRateMeter::default();
        // 120 tokens over 2 s is below the evidence gate unscaled.
        meter.seed(120, 2_000);
        assert!((reading(&meter, 0) - 60.0).abs() < 0.5);
        assert!((reading(&meter, 60_000) - 60.0).abs() < 0.5);
        meter.seed(0, 2_000);
        assert_eq!(meter.rate(0), None);
    }
}
