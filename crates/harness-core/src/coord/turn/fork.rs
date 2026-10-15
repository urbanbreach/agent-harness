use super::*;

impl Worker {
    /// A detached copy for background speculation: same model, tools and prompt
    /// sources, its own cancellation, and no manual reply or turn guidance.
    pub(in crate::coord) fn fork(&self, cancellation: CancellationToken) -> Self {
        Self {
            tool_scope: self.tool_scope.clone(),
            handle: self.handle.clone(),
            provider: Arc::clone(&self.provider),
            permits: Arc::clone(&self.permits),
            profile: Arc::clone(&self.profile),
            prompt_source: self.prompt_source.clone(),
            prompt_date: self.prompt_date.clone(),
            prompt_limits: self.prompt_limits,
            workspace: self.workspace.clone(),
            compaction: self.compaction.clone(),
            tools: self.tools.clone(),
            native_schema: self.native_schema.clone(),
            native: self.native,
            skill_metadata: self.skill_metadata.clone(),
            skill_preload_names: self.skill_preload_names.clone(),
            skill_preloads: self.skill_preloads.clone(),
            registry: Arc::clone(&self.registry),
            permissions: self.permissions.clone(),
            actor: self.actor.clone(),
            session: self.session.clone(),
            cancellation,
            turn: Turn {
                id: self.turn.id.clone(),
                prompt: super::super::prompt::Prompt::default(),
                model: self.turn.model.clone(),
                settings: self.turn.settings.clone(),
                target: self.turn.target.clone(),
                seq: self.turn.seq,
                inherited_selection: self.turn.inherited_selection,
                user_initiated: false,
                manual: None,
            },
            fallbacks: std::collections::VecDeque::new(),
            retry: self.retry.clone(),
            behavior: self.behavior.clone(),
            guidance: guidance::Guidance::default(),
            counter: Arc::clone(&self.counter),
            overlay: self.overlay.clone(),
            parked: self.parked.clone(),
            notes: self.notes.clone(),
        }
    }
}

/// A counter for `model_ref` as `provider` serves it. The provider reports the real wire api
/// and endpoint facts; only when it cannot (mock and test providers) does the reader api follow
/// the model lineage (Anthropic models speak the Anthropic api, OpenAI models the Responses api).
pub(in crate::coord) fn counter_for(
    provider: &dyn harness_providers::Provider,
    model_ref: &str,
) -> Arc<harness_compaction::tokenizer::Counter> {
    use harness_compaction::tokenizer::{classify, Counter, Family, Reader, ReaderApi};
    let model = crate::agent::AgentModelRef::parse(model_ref);
    let request = harness_providers::CompletionRequest {
        provider_id: Some(model.provider_id.clone()),
        model_id: model.model_id.clone(),
        ..Default::default()
    };
    let reader = provider.token_reader(&request).unwrap_or_else(|| {
        let api = match classify(&model.provider_id, &model.model_id).family {
            Family::Anthropic { .. } => ReaderApi::Anthropic,
            Family::OpenAi => ReaderApi::OpenAiResponses,
            _ => ReaderApi::Other,
        };
        Reader {
            api,
            provider: model.provider_id,
            model_id: model.model_id,
            official_endpoint: false,
            prefix_binding: false,
            filter_reasoning_history: false,
        }
    });
    Arc::new(Counter::new(Some(reader)))
}
