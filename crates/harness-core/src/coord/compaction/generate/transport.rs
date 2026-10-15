use super::extract::{cancelled_error, Reader, RetryMode};
use super::*;
use crate::coord::compaction::{CompactionError, CompactionErrorKind};

impl Worker {
    /// The system text a side request sends: the live agent system prompt, then the
    /// summarization guidance as the role instructions.
    pub(super) fn side_system(&self, context: &Context) -> String {
        let guidance = prompts::summarization_system_prompt();
        let effective = self.effective_system(context);
        if effective.is_empty() {
            guidance.to_owned()
        } else {
            format!("{effective}\n\n{guidance}")
        }
    }

    /// Output cap for a side request. Generators pass their exact formula result; only the
    /// provider-validity clamp (model maximum, never zero) applies here, for local and
    /// endpoint requests alike.
    pub(super) fn clamp_output(target: &ResolvedModelTarget, max_tokens: u32) -> u32 {
        target
            .limits
            .max_output_tokens()
            .map_or(max_tokens, |limit| limit.min(max_tokens))
            .max(1)
    }

    /// A summary-side request: effective system prompt plus summarization guidance, one
    /// user message, no tools, the session's thinking selection (including off), a unique
    /// side session identity and the main session's prompt-cache identity and retention.
    pub(super) fn side_request(
        &self,
        target: &ResolvedModelTarget,
        context: &Context,
        user: String,
        max_tokens: u32,
    ) -> CompletionRequest {
        let caps = &target.resolution.capabilities;
        CompletionRequest {
            provider_id: Some(target.provider.clone()),
            model_id: target.model.clone(),
            variant: target.variant.clone(),
            max_tokens: Some(Self::clamp_output(target, max_tokens)),
            reasoning_effort: request::resolve_effort(
                self.turn.settings.reasoning_effort.as_deref(),
                &caps.reasoning_efforts,
                !caps.reasoning_efforts.is_empty(),
            ),
            messages: vec![
                CompletionMessage::text(MessageRole::System, self.side_system(context)),
                CompletionMessage::text(MessageRole::User, user),
            ],
            context: self.side_context(),
            stream: true,
            ..Default::default()
        }
    }

    /// Routing and cache identity shared by every side request.
    pub(super) fn side_context(&self) -> ProviderRequestContext {
        ProviderRequestContext {
            working_directory: Some(self.workspace.clone()),
            session_id: Some(format!(
                "{}:side:{}",
                self.session,
                uuid::Uuid::now_v7().simple()
            )),
            cache_key: Some(self.session.clone()),
            initiator: ProviderRequestInitiator::Agent,
            cache_retention: self.profile.cache_retention,
            main_turn: false,
            ..Default::default()
        }
    }

    fn reader<'a>(&'a self, progress: &'a Progress, preview: bool) -> Reader<'a> {
        Reader {
            handle: &self.handle,
            provider: self.provider.as_ref(),
            permits: &self.permits,
            actor: &self.actor,
            task: &self.turn.id,
            cancel: &progress.cancel,
            preview: preview.then_some(progress),
        }
    }

    /// One summary-side round trip, retrying transient failures per `mode`.
    pub(in crate::coord::compaction) async fn side(
        &self,
        request: CompletionRequest,
        kind: CompactionErrorKind,
        progress: &Progress,
        mode: RetryMode,
        preview: bool,
    ) -> Result<Side, CompactionError> {
        let extracted = self
            .reader(progress, preview)
            .run(request, kind, mode)
            .await?;
        Ok(Side {
            text: extracted.text,
            usage: extracted.usage,
            aborted: extracted.aborted,
            ..Side::default()
        })
    }

    /// Summary text from a configured remote endpoint, authenticated with the candidate's
    /// credential. Failures keep their transport classification.
    #[expect(
        clippy::too_many_arguments,
        reason = "An endpoint request carries its whole transport context"
    )]
    pub(in crate::coord::compaction) async fn endpoint_summary(
        &self,
        endpoint: &str,
        candidate: &Candidate,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        kind: CompactionErrorKind,
        progress: &Progress,
    ) -> Result<Side, CompactionError> {
        // Credentials are resolved per candidate model before the request; without them the
        // request never leaves the process.
        let Some(credential) = candidate.credential.as_ref() else {
            let mut error = CompactionError::new(
                CompactionErrorKind::Endpoint,
                "Remote compaction credentials unavailable",
            );
            error.auth = true;
            error.category = Some(ProviderErrorCategory::MissingCredentials);
            return Err(error);
        };
        let result = tokio::select! {
            biased;
            () = progress.cancel.cancelled() => return Err(cancelled_error(kind)),
            result = harness_providers::custom_endpoint_summary(
                endpoint,
                Some(credential),
                Some(&candidate.model),
                system,
                prompt,
                Some(max_tokens),
                progress.cancel.clone(),
            ) => result,
        };
        match result {
            Ok(summary) => Ok(Side {
                text: summary.summary,
                short_summary: summary.short_summary,
                extra: summary.extra,
                ..Side::default()
            }),
            Err(error) => {
                let mut error = CompactionError::from(error);
                error.kind = CompactionErrorKind::Endpoint;
                Err(error)
            }
        }
    }
}
