use crate::config::ResolvedModelTarget;

/// Owned prompt inputs retained across model switches and provider fallbacks.
/// The front end supplies its section renderer; core owns dispatch-time selection.
#[derive(Debug, Clone)]
pub struct ModelPromptTemplate {
    pub model: ResolvedModelTarget,
    pub configured_prompt: Option<String>,
    pub role_prompt: Option<String>,
    pub instruction_prompt: Option<String>,
    pub extra_rules: Option<String>,
    pub render: fn(&Self, &ResolvedModelTarget, bool) -> String,
}

impl ModelPromptTemplate {
    pub fn recompose_profile(
        &self,
        profile: &mut crate::agent::AgentProfile,
        request: &crate::agent::AgentRequest,
    ) {
        let mut inferred = self.model.clone();
        if request.model_target.is_none() && inferred.model_ref != request.model_ref {
            let model = crate::agent::AgentModelRef::parse(&request.model_ref);
            inferred.model_ref = request.model_ref.clone();
            inferred.provider = model.provider_id;
            inferred.model = model.model_id;
            inferred.resolution = super::resolve_model(super::ModelResolutionInput {
                provider: &inferred.provider,
                model: &inferred.model,
                metadata_family: None,
                input_modalities: &[],
                supports_tool_calls: None,
                supports_reasoning_summaries: None,
            });
        }
        profile.system_prompt = self.compose(
            request.model_target.as_ref().unwrap_or(&inferred),
            profile.toolset.iter().any(|tool| tool == "skill"),
        );
    }

    pub fn compose(&self, model: &ResolvedModelTarget, skill_tool_enabled: bool) -> String {
        let mut prompt = (self.render)(self, model, skill_tool_enabled);
        if let Some(rules) = &self.extra_rules {
            prompt.push_str("\n\n");
            prompt.push_str(rules);
        }
        prompt
    }
}
