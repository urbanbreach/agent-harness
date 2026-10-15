use super::*;
use harness_providers::{
    CompletionRequest, ProviderErrorCategory, ProviderRequestContext, ProviderRequestInitiator,
};

impl Worker {
    pub(super) fn update_prompt(
        &mut self,
        messages: &mut super::super::context::Context,
    ) -> Result<(), CoordinatorError> {
        self.tools = self.registry.definitions(
            &self.profile,
            self.tool_scope.as_deref(),
            &self.turn.model,
            (&self.permissions.0, &self.permissions.1),
        );
        super::super::public_subagent_hooks::apply_native_schema_hints(
            &mut self.tools,
            &self.native_schema,
            self.native,
        );
        let Some(source) = &self.prompt_source else {
            return Ok(());
        };
        let resolution = self.turn.target.as_ref().map_or_else(
            || {
                let model = crate::agent::AgentModelRef::parse(&self.turn.model);
                crate::model_resolution::resolve_model(
                    crate::model_resolution::ModelResolutionInput {
                        provider: &model.provider_id,
                        model: &model.model_id,
                        metadata_family: None,
                        input_modalities: &[],
                        supports_tool_calls: None,
                        supports_reasoning_summaries: None,
                    },
                )
            },
            |target| target.resolution.clone(),
        );
        let mut available = self.registry.prompt_definitions(
            &self.profile,
            self.tool_scope.as_deref(),
            &self.turn.model,
            (&self.permissions.0, &self.permissions.1),
        );
        super::super::public_subagent_hooks::apply_native_schema_hints(
            &mut available,
            &self.native_schema,
            self.native,
        );
        let rendered = source
            .render(&crate::system_prompt::PromptContext {
                workspace: &self.workspace,
                model: &self.turn.model,
                prompt_preset: &resolution.prompt_preset,
                tools: &available,
                direct_tools: &self.tools,
                current_date: &self.prompt_date,
                max_concurrent: self.prompt_limits.0,
                limit_behavior: self.prompt_limits.1,
                behavior: &self.behavior,
            })
            .map_err(|error| CoordinatorError::Invalid(format!("system prompt: {error}")))?;
        if let Some(eval) = self.tools.iter_mut().find(|tool| tool.tool_id == "eval")
            && let Some(original) = available.iter().find(|tool| tool.tool_id == "eval")
        {
            let base = original.description.as_deref().unwrap_or_default();
            eval.description = Some(if rendered.eval_tool_guidance.is_empty() {
                base.to_owned()
            } else {
                format!("{base}\n\n{}", rendered.eval_tool_guidance)
            });
        }
        if let Some(entry) = messages
            .entries
            .first_mut()
            .filter(|e| e.message.role == MessageRole::System)
        {
            entry.message.content = rendered.system;
        } else {
            let mut system = super::super::context::Context::new(&rendered.system);
            system.entries.append(&mut messages.entries);
            messages.entries = system.entries;
        }
        Ok(())
    }
    pub(in crate::coord) fn request(
        &self,
        messages: &super::super::context::Context,
    ) -> CompletionRequest {
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        // Request-only projection: source locations and catalog configuration
        // never enter conversation buffers, finalized state, or durable events.
        let mut request_messages = messages.messages();
        let mut attachments = messages.attachments();
        let mut message_content = std::collections::BTreeMap::new();
        self.project_archives(
            messages,
            &model,
            &mut request_messages,
            &mut attachments,
            &mut message_content,
        );
        self.insert_skill_system(
            &mut request_messages,
            &mut attachments,
            &mut message_content,
        );
        self.apply_inline_imaging(
            messages,
            &model,
            &mut request_messages,
            &mut attachments,
            &mut message_content,
        );
        // Times pair with entries only while the projections kept one message per entry.
        let message_times_ms = if request_messages.len() == messages.entries.len() {
            messages.entries.iter().map(|entry| entry.at_ms).collect()
        } else {
            Vec::new()
        };
        CompletionRequest {
            provider_id: Some(model.provider_id),
            model_id: model.model_id,
            messages: request_messages,
            context: ProviderRequestContext {
                has_media: attachments
                    .values()
                    .flatten()
                    .any(|a| a.mime.starts_with("image/")),
                working_directory: Some(self.workspace.clone()),
                session_id: Some(self.session.clone()),
                request_id: Some(self.turn.id.clone()),
                initiator: if self.turn.user_initiated
                    && messages
                        .entries
                        .last()
                        .is_some_and(|e| e.message.role == MessageRole::User)
                {
                    ProviderRequestInitiator::User
                } else {
                    ProviderRequestInitiator::Agent
                },
                cache_retention: self.profile.cache_retention,
                main_turn: true,
                cache_key: None,
            },
            attachments,
            message_content,
            message_times_ms,
            temperature: self.profile.temperature,
            max_tokens: self
                .turn
                .target
                .as_ref()
                .and_then(|t| t.limits.max_output_tokens()),
            variant: self.turn.settings.variant.clone(),
            reasoning_effort: self.turn.settings.reasoning_effort.clone(),
            text_verbosity: self.turn.settings.text_verbosity.clone(),
            reasoning_summary: self.turn.settings.reasoning_summary.clone(),
            thinking: self.turn.settings.thinking.clone(),
            tools: (!self.tools.is_empty()).then(|| self.tools.clone()),
            stream: true,
            ..Default::default()
        }
    }
    fn selection(
        &self,
        request: &CompletionRequest,
    ) -> Result<Box<crate::session::CanonicalRuntimeSelection>, CoordinatorError> {
        Ok(Box::new(crate::session::CanonicalRuntimeSelection {
            profile: Some(self.profile.name.clone()),
            provider_id: request.provider_id.clone().unwrap_or_default(),
            model_id: request.model_id.clone(),
            variant: request.variant.clone(),
            reasoning_effort: request.reasoning_effort.clone(),
            text_verbosity: request.text_verbosity.clone(),
            reasoning_summary: request.reasoning_summary.clone(),
            thinking: request.thinking.clone(),
            resolved_limits: self
                .turn
                .target
                .as_ref()
                .map(|t| t.limits.clone())
                .unwrap_or_default(),
            profile_tool_shape_digest: digest(&serde_json::to_string(&self.tools)?),
        }))
    }
    pub(in crate::coord) fn request_budget(
        &self,
        request: &CompletionRequest,
    ) -> Result<crate::RequestBudget, CoordinatorError> {
        let unknown = crate::config::ResolvedModelLimits::default();
        let limits = self.turn.target.as_ref().map_or(&unknown, |t| &t.limits);
        limits
            .validate("active model")
            .map_err(|error| CoordinatorError::Invalid(error.to_string()))?;
        let pending = if request
            .messages
            .last()
            .is_some_and(|m| m.role == MessageRole::User)
        {
            request.messages.len() - 1
        } else {
            request.messages.len()
        };
        let semantics = self
            .provider
            .request_budget_semantics(request, pending)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        crate::compute_request_budget(crate::RequestBudgetInput {
            model_limits: limits,
            request_cost: semantics.request_cost,
            requested_output_tokens: request.max_tokens,
            safety_margin_tokens: 0,
            estimated_token_triggers: self.compaction.estimated_token_triggers,
            fallback_input_tokens: self.compaction.fallback_input_tokens,
            output_cap_disposition: semantics.output_cap_disposition,
        })
        .map_err(|e| CoordinatorError::Invalid(e.to_string()))
    }
}

mod dispatch;
mod inline;
mod projection;
mod system;
