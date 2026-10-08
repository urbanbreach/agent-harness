use super::*;
use harness_providers::{CompletionMessage, CompletionRequest, MessageRole};

impl CoordinatorHandle {
    /// A tool-free request with the cell's cancellation, provider capacity and usage accounting.
    pub async fn eval_completion(
        &self,
        parent: String,
        input: Value,
        cancellation: CancellationToken,
    ) -> Result<Value, CoordinatorError> {
        let opts = input
            .get("opts")
            .filter(|v| v.is_object())
            .unwrap_or(&input);
        let prompt = input["prompt"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                CoordinatorError::Invalid("completion requires a nonempty prompt".into())
            })?
            .to_owned();
        let option = |key: &str| opts.get(key).or_else(|| input.get(key));
        let system = option("system")
            .and_then(Value::as_str)
            .unwrap_or("Answer the request.")
            .to_owned();
        let tier = option("model")
            .and_then(Value::as_str)
            .unwrap_or("default")
            .to_owned();
        if !matches!(tier.as_str(), "default" | "smol" | "slow") {
            return Err(CoordinatorError::Invalid(
                "completion model must be default, smol or slow".into(),
            ));
        }
        let schema = option("schema").cloned();
        let task = parent.clone();
        let (provider, permits, actor, parent_cancel, target, model_ref, compaction) = self
            .call(move |s| {
                s.check_eval(&task)?;
                let job = &s.running[&task];
                let agent = job
                    .actor
                    .agent_id
                    .as_ref()
                    .and_then(|id| s.agents.get(id))
                    .ok_or_else(|| {
                        CoordinatorError::Invalid("completion requires an agent session".into())
                    })?;
                let target = if tier == "default" {
                    agent.target.clone()
                } else {
                    Some(
                        s.config
                            .agent_model_targets
                            .get(&tier)
                            .cloned()
                            .ok_or_else(|| {
                                CoordinatorError::Invalid(format!(
                                    "completion tier {tier} needs a configured {tier} agent model"
                                ))
                            })?,
                    )
                };
                let model_ref = target
                    .as_ref()
                    .map_or_else(|| agent.info.model_ref.clone(), |t| t.model_ref.clone());
                Ok((
                    Arc::clone(&s.config.provider),
                    Arc::clone(&s.providers),
                    job.actor.clone(),
                    job.cancellation.clone(),
                    target,
                    model_ref,
                    s.config.compaction.clone(),
                ))
            })
            .await?;
        let model = crate::agent::AgentModelRef::parse(&model_ref);
        let mut request = CompletionRequest {
            provider_id: Some(model.provider_id.clone()),
            model_id: model.model_id.clone(),
            max_tokens: target.as_ref().and_then(|t| t.limits.max_output_tokens()),
            messages: vec![
                CompletionMessage::text(MessageRole::System, system),
                CompletionMessage::text(MessageRole::User, prompt),
            ],
            variant: target.as_ref().and_then(|t| t.variant.clone()),
            reasoning_effort: target.as_ref().and_then(|t| t.reasoning_effort.clone()),
            reasoning_summary: target.as_ref().and_then(|t| t.reasoning_summary.clone()),
            text_verbosity: target.as_ref().and_then(|t| t.text_verbosity.clone()),
            thinking: target.as_ref().and_then(|t| t.thinking.clone()),
            stream: true,
            ..Default::default()
        };
        if let Some(schema) = &schema {
            request.messages[1].content.push_str(&format!(
                "\nRespond only with JSON matching this schema:\n{schema}"
            ));
        }
        let serialized = serde_json::to_string(&request)?;
        if serialized.len() > 1024 * 1024 {
            return Err(CoordinatorError::Invalid(
                "completion request exceeds 1 MiB".into(),
            ));
        }
        let limits = target
            .as_ref()
            .map(|t| t.limits.clone())
            .unwrap_or_default();
        let semantics = provider
            .request_budget_semantics(&request, 1)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        let budget = crate::compute_request_budget(crate::RequestBudgetInput {
            model_limits: &limits,
            request_cost: semantics.request_cost,
            requested_output_tokens: request.max_tokens,
            safety_margin_tokens: 0,
            estimated_token_triggers: compaction.estimated_token_triggers,
            fallback_input_tokens: compaction.fallback_input_tokens,
            output_cap_disposition: semantics.output_cap_disposition,
        })
        .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        if budget.requires_compaction == Some(true) {
            return Err(CoordinatorError::Invalid(
                "completion prompt exceeds the model's input budget".into(),
            ));
        }
        let _permit = tokio::select! {
            () = cancellation.cancelled() => return Err(CoordinatorError::Cancelled(parent)),
            () = parent_cancel.cancelled() => return Err(CoordinatorError::Cancelled(parent)),
            permit = permits.acquire_owned() => permit.map_err(|_| CoordinatorError::Closed)?,
        };
        let (task, owner) = (parent.clone(), actor.clone());
        let request_id = self
            .call(move |s| {
                s.check_eval(&task)?;
                let id = s.id("eval-completion")?;
                s.emit_hooked(
                    owner,
                    Some(task.clone()),
                    EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                        request_id: id.clone().into(),
                        provider_id: model.provider_id,
                        model_id: model.model_id,
                        prompt_summary: "Eval completion".into(),
                        request_digest: runtime::digest(&serialized),
                        metadata: Some(ProviderRequestStartedMetadata {
                            turn_id: Some(task),
                            context_budget: Some(budget),
                            ..Default::default()
                        }),
                    }),
                )?;
                Ok(id)
            })
            .await?;
        request.context.request_id = Some(request_id.clone());
        let (mut progress, mut aborted_usage) = (false, None);
        let response = tokio::select! {
            () = cancellation.cancelled() => Err(CoordinatorError::Cancelled(parent.clone())),
            response = super::super::streaming::read(self, provider.as_ref(), &actor, &parent, &request_id, request, &parent_cancel, false, false, &mut progress, &mut aborted_usage) => response,
        };
        let response = self
            .call(move |s| {
                s.emit_hooked(
                    actor,
                    Some(parent.clone()),
                    EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                        request_id: request_id.into(),
                        finish_reason: match &response {
                            Ok(_) => "stop",
                            Err(CoordinatorError::Cancelled(_)) => "cancelled",
                            Err(_) => "error",
                        }
                        .into(),
                        output_digest: response.as_ref().ok().map(|r| runtime::digest(&r.text)),
                        usage: response.as_ref().ok().and_then(|r| r.usage.clone()),
                        metadata: Some(ProviderRequestFinishedMetadata {
                            turn_id: Some(parent),
                            ..Default::default()
                        }),
                    }),
                )?;
                response
            })
            .await?;
        if !response.calls.is_empty() || response.text.trim().is_empty() {
            return Err(CoordinatorError::Invalid(
                "completion returned no text or attempted a tool call".into(),
            ));
        }
        let details = serde_json::json!({"model":model_ref,"structured":schema.is_some()});
        if schema.is_some() {
            let value: Value = serde_json::from_str(&response.text)?;
            Ok(serde_json::json!({"value":value,"details":details}))
        } else {
            Ok(serde_json::json!({"text":response.text,"details":details}))
        }
    }
}
