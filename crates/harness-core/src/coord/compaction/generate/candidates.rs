//! Summarizer candidate assembly and the shared candidate walk.
use super::extract::from_coordinator;
use super::walk;
use super::*;
use crate::coord::compaction::{CompactionError, CompactionErrorKind};
use harness_providers::CompactionCredential;

/// One model that may write a summary, with the route facts the walk needs.
#[derive(Debug, Clone)]
pub(in crate::coord::compaction) struct Candidate {
    pub provider: String,
    pub model: String,
    pub window: Option<u32>,
    /// The provider offers a native compaction route for this model under the session settings.
    pub native_capable: bool,
    /// Resolved by the walk before the attempt runs; `None` while the list is being built.
    pub credential: Option<CompactionCredential>,
}

/// Where a recorded native route restricts candidates: same provider, native-capable and,
/// when `model` is set, exactly this model (provider-bound prefixes cannot move).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord::compaction) struct NativeBoundary {
    pub provider: String,
    pub model: Option<String>,
}

/// Which loop drives the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord::compaction) enum WalkMode {
    /// The maintenance loop: retries with the session policy and advances models.
    Automatic,
    /// A manual pass: no outer backoff, non-auth failures propagate.
    Manual,
    /// Background speculation: same rules as manual.
    Speculative,
}

fn key(provider: &str, model: &str) -> String {
    format!("{provider}/{model}")
}

impl NativeBoundary {
    fn admits(&self, candidate: &Candidate) -> bool {
        candidate.provider == self.provider
            && candidate.native_capable
            && self
                .model
                .as_ref()
                .is_none_or(|model| *model == candidate.model)
    }
}

impl Worker {
    /// The request a credential or lane lookup describes for one candidate.
    fn candidate_request(&self, provider: &str, model: &str) -> CompletionRequest {
        CompletionRequest {
            provider_id: Some(provider.to_owned()),
            model_id: model.to_owned(),
            context: ProviderRequestContext {
                session_id: Some(self.session.clone()),
                initiator: ProviderRequestInitiator::Agent,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn candidate_for(&self, target: &ResolvedModelTarget) -> Candidate {
        let request = self.candidate_request(&target.provider, &target.model);
        let lanes = self
            .provider
            .native_compaction_lanes(&request, &self.native_lane_settings(&request));
        Candidate {
            provider: target.provider.clone(),
            model: target.model.clone(),
            window: target.limits.context_window_tokens(),
            native_capable: !lanes.is_empty(),
            credential: None,
        }
    }

    /// The configured summary model for the live model: the per-model entry, else the global
    /// one. `@smol` and `@slow` name model roles; a bare id means the live provider's model.
    async fn preferred_target(
        &self,
        live: &ResolvedModelTarget,
    ) -> Result<Option<ResolvedModelTarget>, CoordinatorError> {
        let settings = &self.compaction;
        let configured = settings
            .summary_models
            .get(&format!("{}/{}", live.provider, live.model))
            .or_else(|| {
                settings
                    .summary_models
                    .get(&format!("{}:{}", live.provider, live.model))
            })
            .or(settings.summary_model.as_ref());
        let Some(raw) = configured
            .map(|value| value.trim())
            .filter(|v| !v.is_empty())
        else {
            return Ok(None);
        };
        let reference = if raw.starts_with('@') {
            let roles = self
                .handle
                .call(|s| Ok(s.config.subagents.model_roles.clone()))
                .await?;
            match raw {
                "@smol" => roles.smol,
                "@slow" => roles.slow,
                _ => None,
            }
        } else {
            Some(raw.to_owned())
        };
        let Some(reference) = reference.map(|value| value.trim().to_owned()) else {
            return Ok(None);
        };
        if reference.is_empty() {
            return Ok(None);
        }
        if let Some(target) = self.resolve_target(&reference).await? {
            return Ok(Some(target));
        }
        self.resolve_target(&format!("{}:{reference}", live.provider))
            .await
    }

    /// Summarizer targets in walk order with their candidates: the configured summary model,
    /// the live model, `@smol`, `@slow`, then the single largest-window catalog model not
    /// yet seen. A native boundary keeps only same-provider native-capable routes.
    pub(in crate::coord::compaction) async fn summary_targets(
        &self,
        native_boundary: Option<&NativeBoundary>,
    ) -> Result<Vec<(Candidate, ResolvedModelTarget)>, CompactionError> {
        self.summary_targets_inner(native_boundary)
            .await
            .map_err(|error| from_coordinator(CompactionErrorKind::Summary, error))
    }

    async fn summary_targets_inner(
        &self,
        native_boundary: Option<&NativeBoundary>,
    ) -> Result<Vec<(Candidate, ResolvedModelTarget)>, CoordinatorError> {
        let live = self.live_target().await?;
        let mut chain = Chain {
            worker: self,
            boundary: native_boundary,
            seen: std::collections::HashSet::new(),
            out: Vec::new(),
        };
        if let Some(preferred) = self.preferred_target(&live).await? {
            chain.push(preferred);
        }
        chain.push(live);
        let (roles, mut catalog) = self
            .handle
            .call(|s| {
                Ok((
                    s.config.subagents.model_roles.clone(),
                    s.config
                        .model_catalog
                        .iter()
                        .filter(|e| e.limits.context_window_tokens().is_some())
                        .map(catalog_target)
                        .collect::<Vec<_>>(),
                ))
            })
            .await?;
        for role in [roles.smol, roles.slow].into_iter().flatten() {
            if let Some(target) = self.resolve_target(role.trim()).await? {
                chain.push(target);
            }
        }
        // Stable sort keeps catalog order among equal windows. Only the first unseen
        // model is examined, even if the boundary then rejects it.
        catalog.sort_by_key(|target| std::cmp::Reverse(target.limits.context_window_tokens()));
        if let Some(target) = catalog
            .into_iter()
            .find(|target| !chain.seen.contains(&key(&target.provider, &target.model)))
        {
            chain.push(target);
        }
        Ok(chain.out)
    }

    /// Summarizer candidates without their resolved targets.
    pub(in crate::coord::compaction) async fn summary_candidates(
        &self,
        native_boundary: Option<&NativeBoundary>,
    ) -> Result<Vec<Candidate>, CompactionError> {
        Ok(self
            .summary_targets(native_boundary)
            .await?
            .into_iter()
            .map(|(candidate, _)| candidate)
            .collect())
    }

    /// The credentials failure shown when no candidate could authenticate.
    pub(in crate::coord::compaction) fn credentials_error(&self) -> CompactionError {
        let message = if self.turn.model.is_empty() {
            "Compaction requires a model with usable credentials, but no authenticated compaction model is available.".to_owned()
        } else {
            let live = crate::agent::AgentModelRef::parse(&self.turn.model);
            format!(
                "Compaction requires usable credentials for {provider}/{model}. Configure {provider} credentials or assign an authenticated fallback role such as model_roles.smol.",
                provider = live.provider_id,
                model = live.model_id,
            )
        };
        let mut error = CompactionError::new(CompactionErrorKind::Summary, message);
        error.auth = true;
        error
    }

    /// Walks `candidates` until `attempt` succeeds; see [`walk::walk`] for the rules. The
    /// credential preflight asks the provider for a session-scoped credential.
    pub(in crate::coord::compaction) async fn walk_candidates<T, F, Fut>(
        &self,
        mode: WalkMode,
        cancel: &CancellationToken,
        candidates: Vec<Candidate>,
        attempt: F,
    ) -> Result<T, CompactionError>
    where
        F: FnMut(&Candidate, u32) -> Fut,
        Fut: Future<Output = Result<T, CompactionError>>,
    {
        let plan = walk::WalkPlan {
            mode,
            retry: &self.compaction.retry,
            cancel,
            credentials: self.credentials_error(),
        };
        let resolve = |candidate: &Candidate| {
            let request = self.candidate_request(&candidate.provider, &candidate.model);
            let provider = Arc::clone(&self.provider);
            let cancel = cancel.clone();
            async move {
                provider
                    .compaction_credential(&request, cancel)
                    .await
                    .map_err(CompactionError::from)
            }
        };
        walk::walk(&plan, candidates, resolve, attempt).await
    }
}

/// The ordered, de-duplicated summarizer chain under construction. A model counts as seen
/// before the native boundary can reject it, so a later fallback never reintroduces it.
struct Chain<'a> {
    worker: &'a Worker,
    boundary: Option<&'a NativeBoundary>,
    seen: std::collections::HashSet<String>,
    out: Vec<(Candidate, ResolvedModelTarget)>,
}

impl Chain<'_> {
    fn push(&mut self, target: ResolvedModelTarget) {
        if !self.seen.insert(key(&target.provider, &target.model)) {
            return;
        }
        let candidate = self.worker.candidate_for(&target);
        if self
            .boundary
            .is_none_or(|boundary| boundary.admits(&candidate))
        {
            self.out.push((candidate, target));
        }
    }
}
