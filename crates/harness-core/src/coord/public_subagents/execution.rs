use super::*;
use harness_providers::MessageRole;

mod definition;
mod preparation;
mod skills;

impl Runtime {
    pub(super) fn native_model_window(&self, model: &str) -> u64 {
        self.native_model_target(model)
            .and_then(|target| target.limits.context_window.tokens)
            .map_or(0, u64::from)
    }

    fn native_model_target(&self, model: &str) -> Option<ResolvedModelTarget> {
        self.config
            .agent_model_targets
            .values()
            .find(|target| target.model_ref == model || target.model == model)
            .cloned()
            .or_else(|| {
                let parsed = crate::agent::AgentModelRef::parse(model);
                let entry = self.config.model_catalog.iter().find(|entry| {
                    entry.model == model
                        || entry.model == parsed.model_id
                            && (entry.provider == parsed.provider_id || !model.contains([':', '/']))
                })?;
                Some(ResolvedModelTarget {
                    model_ref: format!("{}:{}", entry.provider, entry.model),
                    provider: entry.provider.clone(),
                    model: entry.model.clone(),
                    variant: entry.variant.clone(),
                    reasoning_effort: entry.reasoning_effort.clone(),
                    text_verbosity: entry.text_verbosity.clone(),
                    reasoning_summary: None,
                    thinking: entry.thinking.clone(),
                    limits: entry.limits.clone(),
                    resolution: entry.resolution.clone(),
                    catalog_entry: Some(Box::new(entry.clone())),
                })
            })
    }
}
