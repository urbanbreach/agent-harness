use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio_stream::{self as stream, StreamExt};

use crate::cassette::{
    CassetteError, CassetteInteraction, CassetteMode, ProviderCassette, CASSETTE_VERSION,
};
use crate::{
    CompletionRequest, Provider, ProviderBudgetSemantics, ProviderEventStream,
    ProviderRequestCostError, ProviderStreamEvent,
};

#[derive(Debug)]
struct CassetteState {
    cassette: ProviderCassette,
    cursor: usize,
    record: bool,
}

/// Provider wrapper that replays or records provider-level completion events from a cassette.
///
/// Matching is intentionally sequential: call N must match interaction N. This makes retry,
/// fallback, and polling behavior observable instead of hidden behind content-keyed dispatch.
pub struct RecordedProvider<P> {
    inner: P,
    path: PathBuf,
    state: Mutex<CassetteState>,
}

impl<P> RecordedProvider<P> {
    pub fn new(
        inner: P,
        path: impl Into<PathBuf>,
        mode: CassetteMode,
    ) -> Result<Self, CassetteError> {
        let ci = std::env::var_os("CI").is_some_and(|value| !value.is_empty() && value != "0");
        Self::with_ci(inner, path, mode, ci)
    }

    pub fn with_ci(
        inner: P,
        path: impl Into<PathBuf>,
        mode: CassetteMode,
        ci: bool,
    ) -> Result<Self, CassetteError> {
        let path = path.into();
        let mode = mode.resolve_for_ci(ci);
        let exists = path.exists();
        let cassette = match mode {
            CassetteMode::Replay if !exists => {
                return Err(CassetteError::MissingReplayCassette { path });
            }
            CassetteMode::Replay => ProviderCassette::read_from(&path)?,
            CassetteMode::Auto if exists => ProviderCassette::read_from(&path)?,
            CassetteMode::Record | CassetteMode::Auto => ProviderCassette::new(Vec::new()),
        };
        let record =
            matches!(mode, CassetteMode::Record) || matches!(mode, CassetteMode::Auto) && !exists;
        Ok(Self {
            inner,
            path,
            state: Mutex::new(CassetteState {
                cassette,
                cursor: 0,
                record,
            }),
        })
    }

    fn replay(&self, req: CompletionRequest) -> Result<Vec<ProviderStreamEvent>, CassetteError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let index = state.cursor;
        let Some(interaction) = state.cassette.interactions.get(index) else {
            return Err(CassetteError::Exhausted {
                count: state.cassette.interactions.len(),
            });
        };
        if interaction.request != req {
            return Err(CassetteError::RequestMismatch {
                index,
                expected: crate::cassette::compact_json(&interaction.request),
                actual: crate::cassette::compact_json(&req),
            });
        }
        let events = interaction.events.clone();
        state.cursor += 1;
        Ok(events)
    }

    fn append_recording(
        &self,
        request: CompletionRequest,
        events: Vec<ProviderStreamEvent>,
    ) -> Result<(), CassetteError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state
            .cassette
            .interactions
            .push(CassetteInteraction::new(request, events));
        state.cassette.write_to(&self.path)
    }
}

#[async_trait]
impl<P> Provider for RecordedProvider<P>
where
    P: Provider + Send + Sync,
{
    fn request_budget_semantics(
        &self,
        request: &CompletionRequest,
        pending_prompt_index: usize,
    ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
        self.inner
            .request_budget_semantics(request, pending_prompt_index)
    }

    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let record = self.state.lock().unwrap_or_else(|e| e.into_inner()).record;
        if !record {
            return match self.replay(req) {
                Ok(events) => Box::pin(stream::iter(events)),
                Err(err) => Box::pin(stream::iter(vec![ProviderStreamEvent::error(
                    err.to_string(),
                )])),
            };
        }

        let events = self
            .inner
            .stream_completion(req.clone())
            .await
            .collect::<Vec<_>>()
            .await;
        match self.append_recording(req, events.clone()) {
            Ok(()) => Box::pin(stream::iter(events)),
            Err(err) => Box::pin(stream::iter(vec![ProviderStreamEvent::error(
                err.to_string(),
            )])),
        }
    }
}
