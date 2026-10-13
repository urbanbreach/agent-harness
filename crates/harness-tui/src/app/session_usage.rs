//! Session cost, priced per finished provider request, and live generation throughput.
use std::collections::BTreeMap;
use std::time::Instant;

use harness_core::config::{BilledTokens, ResolvedModelCatalogEntry};
use harness_core::event::{ProviderRequestFinishedEvent, ProviderRequestStartedEvent};

use super::AppState;

mod token_rate;

/// Shorter request spans are noise, not a rate.
const MIN_SEED_SPAN_MS: u64 = 100;

#[derive(Default)]
pub(crate) struct SessionUsage {
    /// Provider, model and start time of requests still in flight.
    started: BTreeMap<String, RequestStart>,
    /// This session's own requests.
    own: Meter,
    /// Each subagent's requests, by child id.
    children: BTreeMap<String, Meter>,
    /// The newest event's stream time and when it arrived, so the open request's stream
    /// time keeps running between deltas.
    observed: Option<(u64, Instant)>,
}

/// One agent's generation rate, the open request whose streamed deltas feed it, and the
/// spend of its finished requests.
#[derive(Default)]
struct Meter {
    rate: token_rate::TokenRateMeter,
    request: Option<String>,
    /// Requests priced when they finished.
    saved: Spend,
    /// Requests recorded without a price, repriced at today's prices: kept per request, with
    /// its start in Unix milliseconds, because a long prompt reprices its whole request and
    /// peak hours price by start time. A request with no recorded start has empty keys.
    billed: BTreeMap<(String, String), Vec<(BilledTokens, Option<i64>)>>,
}

impl Meter {
    fn spend(&self, models: &[ResolvedModelCatalogEntry]) -> Spend {
        let mut spend = self.saved;
        for ((provider, model), requests) in &self.billed {
            let cost = models
                .iter()
                .find(|entry| entry.provider == *provider && entry.model == *model)
                .and_then(|entry| entry.cost.as_ref());
            for (tokens, started_at_ms) in requests {
                match cost.and_then(|cost| cost.picodollars(tokens, *started_at_ms)) {
                    Some(picodollars) => {
                        spend.picodollars += picodollars;
                        spend.priced = true;
                    }
                    None => spend.unpriced = true,
                }
            }
        }
        spend
    }
}

/// Priced spend, and whether any usage had a price or lacked one.
#[derive(Clone, Copy, Default)]
struct Spend {
    picodollars: u128,
    priced: bool,
    unpriced: bool,
}

impl Spend {
    fn add(self, other: Self) -> Self {
        Self {
            picodollars: self.picodollars + other.picodollars,
            priced: self.priced || other.priced,
            unpriced: self.unpriced || other.unpriced,
        }
    }

    /// `0.123`, with `+` when part of the usage has no price.
    fn amount(&self) -> String {
        let mills = (self.picodollars + 500_000_000) / 1_000_000_000;
        let partial = if self.unpriced { "+" } else { "" };
        format!("{}.{:03}{partial}", mills / 1000, mills % 1000)
    }
}

struct RequestStart {
    provider: String,
    model: String,
    mono_ms: u64,
    started_at_ms: Option<i64>,
}

impl SessionUsage {
    /// `child` names the subagent that sent the request, `None` for this session's own;
    /// `ts` is the event's RFC 3339 time. Historical requests only price and seed.
    pub(crate) fn request_started(
        &mut self,
        event: &ProviderRequestStartedEvent,
        mono_ms: u64,
        ts: Option<&str>,
        child: Option<&str>,
        historical: bool,
    ) {
        let request_id = event.request_id.to_string();
        if !historical {
            let meter = self.meter_mut(child);
            meter.rate.begin(mono_ms);
            meter.request = Some(request_id.clone());
        }
        let started_at_ms = ts.and_then(harness_core::clock::rfc3339_unix_ms);
        self.started.insert(
            request_id,
            RequestStart {
                provider: event.provider_id.clone(),
                model: event.model_id.clone(),
                mono_ms,
                started_at_ms,
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
        let meter = self.meter_mut(child);
        if let Some(picodollars) = metadata.and_then(|m| m.cost_picodollars) {
            meter.saved.picodollars += u128::from(picodollars);
            meter.saved.priced = true;
            return;
        }
        let (model, started_at_ms) = start.map_or_else(Default::default, |start| {
            ((start.provider, start.model), start.started_at_ms)
        });
        let tokens = BilledTokens::new(
            usage,
            metadata.and_then(|m| m.cache_read_tokens).unwrap_or(0),
            metadata.and_then(|m| m.cache_write_tokens).unwrap_or(0),
            metadata.and_then(|m| m.cache_write_1h_tokens).unwrap_or(0),
        );
        meter
            .billed
            .entry(model)
            .or_default()
            .push((tokens, started_at_ms));
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

    /// `$0.123` for this session's own requests, then `(+0.045)` for its subagents, nested
    /// ones included. `+` after an amount marks usage without a known price; `(sub)` marks a
    /// subscription sign-in, which is not billed per token.
    pub(crate) fn session_cost_label(&self) -> Option<String> {
        let models = self.launch_metadata.resolved_models();
        let usage = &self.projection.usage;
        let own = usage.own.spend(models);
        let children = usage
            .children
            .values()
            .map(|meter| meter.spend(models))
            .fold(Spend::default(), Spend::add);
        if !own.priced && !children.priced {
            return None;
        }
        let subagents = if children.priced || children.unpriced {
            format!(" (+{})", children.amount())
        } else {
            String::new()
        };
        let subscription = if self.launch_metadata.uses_oauth_authentication() {
            " (sub)"
        } else {
            ""
        };
        Some(format!("${}{subagents}{subscription}", own.amount()))
    }

    /// `$0.045` for subagent `child`'s own requests, once any of them has a price.
    pub(crate) fn subagent_cost_label(&self, child: &str) -> Option<String> {
        let spend = self
            .projection
            .usage
            .children
            .get(child)?
            .spend(self.launch_metadata.resolved_models());
        spend.priced.then(|| format!("${}", spend.amount()))
    }
}
