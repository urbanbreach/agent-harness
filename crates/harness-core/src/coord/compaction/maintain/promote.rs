//! Context promotion: running this turn on the configured larger-context model instead of
//! compacting.
use super::*;
use crate::coord::context::Entry;
use harness_providers::{CompletionRequest, MessageRole, ProviderRequestContext};

/// Which trigger asks for a promotion. Idle maintenance never promotes, and a trusted
/// byte-size rejection never asks: a larger token window cannot raise a body limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum PromoteWhy {
    PreRequest,
    PostTurn,
    Overflow,
    Incomplete,
}

impl PromoteWhy {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PreRequest => "pre-request",
            Self::PostTurn => "post-turn",
            Self::Overflow => "overflow",
            Self::Incomplete => "incomplete",
        }
    }
}

/// Entries produced by the model the worker runs on. An entry without recorded provenance
/// counts as the active model's own.
pub(super) fn is_active_model(active: &str, entry: &Entry) -> bool {
    entry.model.as_deref().is_none_or(|recorded| {
        let (recorded, active) = (
            crate::agent::AgentModelRef::parse(recorded),
            crate::agent::AgentModelRef::parse(active),
        );
        recorded.provider_id == active.provider_id && recorded.model_id == active.model_id
    })
}

/// The newest assistant entry of `context`.
pub(super) fn latest_assistant(context: &Context) -> Option<&Entry> {
    context
        .entries
        .iter()
        .rev()
        .find(|entry| entry.message.role == MessageRole::Assistant)
}

impl Worker {
    /// The configured promotion target for the current model, spelled either way.
    fn promotion_target_ref(&self) -> Option<String> {
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        let targets = &self.compaction.promotion_targets;
        [
            self.turn.model.clone(),
            format!("{}/{}", model.provider_id, model.model_id),
            format!("{}:{}", model.provider_id, model.model_id),
        ]
        .iter()
        .find_map(|key| targets.get(key).cloned())
    }

    /// Whether the target's provider has a usable credential for this session. Keyless
    /// providers count as usable; a failed lookup is an error, not a no.
    async fn promotion_credential(
        &self,
        target: &ResolvedModelTarget,
    ) -> Result<bool, CoordinatorError> {
        let request = CompletionRequest {
            provider_id: Some(target.provider.clone()),
            model_id: target.model.clone(),
            context: ProviderRequestContext {
                session_id: Some(self.session.clone()),
                ..Default::default()
            },
            ..Default::default()
        };
        let credential = self
            .provider
            .compaction_credential(&request, self.cancellation.clone())
            .await
            .map_err(super::super::generate::native_error)?;
        Ok(credential.is_some())
    }

    /// Switches this turn to the current model's configured larger-context target.
    /// Returns true when the model changed, so the caller rebuilds its request. The switch
    /// is ephemeral: the operator's selection is never rewritten. Any reason the target
    /// cannot serve returns false and lets compaction run.
    pub(in crate::coord) async fn promote(
        &mut self,
        why: PromoteWhy,
    ) -> Result<bool, CoordinatorError> {
        if !self.compaction.context_promotion {
            return Ok(false);
        }
        let Some(current) = self
            .turn
            .target
            .as_ref()
            .and_then(|target| target.limits.context_window_tokens())
            .filter(|window| *window > 0)
        else {
            return Ok(false);
        };
        let Some(reference) = self.promotion_target_ref() else {
            return Ok(false);
        };
        let Some(target) = self.resolve_target(&reference).await? else {
            return Ok(false);
        };
        let larger = target
            .limits
            .context_window_tokens()
            .is_some_and(|window| window > current);
        let active = crate::agent::AgentModelRef::parse(&self.turn.model);
        if !larger || (target.provider == active.provider_id && target.model == active.model_id) {
            return Ok(false);
        }
        if !self.promotion_credential(&target).await? {
            return Ok(false);
        }
        let (provider, model) = (target.provider.clone(), target.model.clone());
        self.guidance.operator_model.get_or_insert_with(|| {
            (
                self.turn.model.clone(),
                self.turn.settings.clone(),
                self.turn.target.clone(),
            )
        });
        self.counter = crate::coord::turn::counter_for(self.provider.as_ref(), &target.model_ref);
        self.turn.model = target.model_ref.clone();
        self.turn.settings = (&target).into();
        self.turn.target = Some(target);
        tracing::debug!(why = why.as_str(), %provider, %model, "context promotion");
        self.notice(
            crate::event::NoticeSeverity::Info,
            format!("Switched to {provider}/{model} for a larger context window."),
        )
        .await?;
        Ok(true)
    }
}

impl Worker {
    /// Puts the operator's model back after a turn that ran promoted, and records that
    /// selection again so the journal never keeps the promotion as the user's choice.
    pub(in crate::coord) async fn restore_operator_model(
        &mut self,
    ) -> Result<(), CoordinatorError> {
        let Some((model, settings, target)) = self.guidance.operator_model.take() else {
            return Ok(());
        };
        let agent = self.actor.agent_id.clone().unwrap_or_default();
        self.handle
            .call(move |s| {
                let state = s
                    .agents
                    .get_mut(&agent)
                    .ok_or_else(|| CoordinatorError::UnknownAgent(agent.clone()))?;
                state.info.model_ref = model;
                state.settings = settings;
                state.target = target;
                s.record_selection(&agent)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_match_the_active_model_by_provider_and_id() {
        let entry = |model: Option<&str>| {
            let mut entry = Entry::new(
                harness_providers::CompletionMessage::text(MessageRole::Assistant, "a"),
                1,
                None,
            );
            entry.model = model.map(std::sync::Arc::from);
            entry
        };
        for (recorded, active, same) in [
            (Some("anthropic/opus"), "anthropic:opus", true),
            (Some("anthropic:opus"), "anthropic/opus", true),
            (Some("anthropic/sonnet"), "anthropic:opus", false),
            (Some("openai/opus"), "anthropic:opus", false),
            (None, "anthropic:opus", true),
        ] {
            assert_eq!(
                is_active_model(active, &entry(recorded)),
                same,
                "{recorded:?}"
            );
        }
    }
}
