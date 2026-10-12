//! Session cost, folded once per finished provider request, and live generation throughput.
use std::collections::BTreeMap;
use std::time::Instant;

use harness_core::config::BilledTokens;
use harness_core::event::{ProviderRequestFinishedEvent, ProviderRequestStartedEvent};

use super::AppState;

mod token_rate;

/// Shorter request spans are noise, not a rate.
const MIN_SEED_SPAN_MS: u64 = 100;

#[derive(Default)]
pub(crate) struct SessionUsage {
    /// Provider, model and start time of requests still in flight.
    started: BTreeMap<String, RequestStart>,
    /// Billed tokens by provider and model; a request with no recorded start has empty keys.
    by_model: BTreeMap<(String, String), BilledTokens>,
    /// Generation rate of this session's own requests.
    own: Meter,
    /// Generation rate of each subagent, by child id.
    children: BTreeMap<String, Meter>,
    /// The newest event's stream time and when it arrived, so the open request's stream
    /// time keeps running between deltas.
    observed: Option<(u64, Instant)>,
}

/// A generation-rate meter and the open request whose streamed deltas feed it.
#[derive(Default)]
struct Meter {
    rate: token_rate::TokenRateMeter,
    request: Option<String>,
}

struct RequestStart {
    provider: String,
    model: String,
    mono_ms: u64,
}

impl SessionUsage {
    /// `child` names the subagent that sent the request, `None` for this session's own.
    /// Historical requests only price and seed.
    pub(crate) fn request_started(
        &mut self,
        event: &ProviderRequestStartedEvent,
        mono_ms: u64,
        child: Option<&str>,
        historical: bool,
    ) {
        let request_id = event.request_id.to_string();
        if !historical {
            let meter = self.meter_mut(child);
            meter.rate.begin(mono_ms);
            meter.request = Some(request_id.clone());
        }
        self.started.insert(
            request_id,
            RequestStart {
                provider: event.provider_id.clone(),
                model: event.model_id.clone(),
                mono_ms,
            },
        );
    }

    /// Streamed text, reasoning or tool input of a live provider request.
    pub(crate) fn request_delta(&mut self, request_id: &str, bytes: usize, mono_ms: u64) {
        if let Some(meter) = self.metering(request_id) {
            meter.rate.push(bytes, mono_ms);
        }
    }

    pub(crate) fn request_finished(
        &mut self,
        event: &ProviderRequestFinishedEvent,
        mono_ms: u64,
        child: Option<&str>,
        historical: bool,
    ) {
        let request_id = event.request_id.as_str();
        let start = self.started.remove(request_id);
        let usage = event.usage.as_ref();
        if let Some(meter) = self.metering(request_id) {
            meter.request = None;
            meter
                .rate
                .end(usage.map(|usage| usage.completion_tokens), mono_ms);
        } else if historical && let (Some(usage), Some(start)) = (usage, start.as_ref()) {
            // A resumed session reads its last request's pace until new requests blend in.
            let span_ms = mono_ms.saturating_sub(start.mono_ms);
            if usage.completion_tokens > 0 && span_ms >= MIN_SEED_SPAN_MS {
                self.meter_mut(child)
                    .rate
                    .seed(usage.completion_tokens, span_ms);
            }
        }
        let Some(usage) = usage else {
            return;
        };
        let metadata = event.metadata.as_ref();
        let model = start.map_or_else(Default::default, |start| (start.provider, start.model));
        self.by_model.entry(model).or_default().add(
            usage,
            metadata.and_then(|m| m.cache_read_tokens).unwrap_or(0),
            metadata.and_then(|m| m.cache_write_tokens).unwrap_or(0),
        );
    }

    fn meter_mut(&mut self, child: Option<&str>) -> &mut Meter {
        match child {
            Some(child) => self.children.entry(child.to_owned()).or_default(),
            None => &mut self.own,
        }
    }

    /// The meter fed by open request `request_id`.
    fn metering(&mut self, request_id: &str) -> Option<&mut Meter> {
        std::iter::once(&mut self.own)
            .chain(self.children.values_mut())
            .find(|meter| meter.request.as_deref() == Some(request_id))
    }

    /// Pairs a live event's stream time with its arrival.
    pub(crate) fn observe(&mut self, mono_ms: u64, now: Instant) {
        if self.observed.is_none_or(|(latest, _)| mono_ms >= latest) {
            self.observed = Some((mono_ms, now));
        }
    }
}
impl AppState {
    pub(crate) fn live_elapsed_since(&self, started_ms: u64) -> u64 {
        self.projection.usage.observed.map_or(0, |(mono_ms, at)| {
            let since = self.now().saturating_duration_since(at).as_millis();
            mono_ms
                .saturating_add(u64::try_from(since).unwrap_or(u64::MAX))
                .saturating_sub(started_ms)
        })
    }
}

impl AppState {
    /// `42.1 tok/s` of this session, or of subagent `child`: live while a request streams,
    /// the last reading between requests.
    pub(crate) fn generation_rate_label(&self, child: Option<&str>) -> Option<String> {
        let usage = &self.projection.usage;
        let meter = match child {
            Some(child) => usage.children.get(child)?,
            None => &usage.own,
        };
        let now_ms = usage.observed.map_or(0, |(mono_ms, at)| {
            let since = self.now().saturating_duration_since(at).as_millis();
            mono_ms.saturating_add(u64::try_from(since).unwrap_or(u64::MAX))
        });
        let rate = meter.rate.rate(now_ms)?;
        Some(format!("{rate:.1} tok/s"))
    }

    /// `$0.123` across every model the session used. `+` marks usage without a known
    /// price; `(sub)` marks a subscription sign-in, which is not billed per token.
    pub(crate) fn session_cost_label(&self) -> Option<String> {
        let models = self.launch_metadata.resolved_models();
        let mut picodollars = 0u128;
        let mut priced = false;
        let mut unpriced = false;
        for ((provider, model), tokens) in &self.projection.usage.by_model {
            let cost = models
                .iter()
                .find(|entry| entry.provider == *provider && entry.model == *model)
                .and_then(|entry| entry.cost);
            match cost {
                Some(cost) => {
                    picodollars += cost.picodollars(tokens);
                    priced = true;
                }
                None => unpriced = true,
            }
        }
        if !priced {
            return None;
        }
        let mills = (picodollars + 500_000_000) / 1_000_000_000;
        let partial = if unpriced { "+" } else { "" };
        let subscription = if self.launch_metadata.uses_oauth_authentication() {
            " (sub)"
        } else {
            ""
        };
        Some(format!(
            "${}.{:03}{partial}{subscription}",
            mills / 1000,
            mills % 1000
        ))
    }
}
