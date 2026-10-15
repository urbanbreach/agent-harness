use super::extract::{into_coordinator, RetryMode};
use super::*;
use crate::coord::compaction::{CompactionError, CompactionErrorKind};
use std::path::PathBuf;

/// A generated handoff document and where it was saved, when it was.
#[allow(
    dead_code,
    reason = "The provider-abort flag is carried for callers that classify it"
)]
pub(in crate::coord::compaction) struct HandoffDocument {
    pub text: String,
    pub usage: Option<CompletionUsage>,
    pub saved_path: Option<String>,
    /// The provider reported its own abort without any host cancellation.
    pub aborted: bool,
}

impl Worker {
    /// A handoff document written by the live model from its own (cached) context.
    ///
    /// The request is the live request (system prompt, tools and the request-only archive
    /// projection, so the cached prefix is unchanged) plus the handoff instruction, sent
    /// with tool choice none and once more with auto when a provider rejects that. Each
    /// lane keeps the summarizer's own default retry, whatever the surrounding pass does.
    ///
    /// `None` means the model returned no text. Only `auto_triggered` documents with the
    /// save setting on are written to disk, at generation time, even if the pass is later
    /// abandoned.
    pub(in crate::coord::compaction) async fn handoff_document(
        &self,
        context: &Context,
        focus: Option<&str>,
        auto_triggered: bool,
        progress: &Progress,
    ) -> Result<Option<HandoffDocument>, CoordinatorError> {
        let kind = CompactionErrorKind::Handoff;
        let task = self.turn.id.as_str();
        let mut request = self.request(context);
        if self.turn.model.is_empty() || request.model_id.is_empty() {
            return Err(CoordinatorError::Compaction(CompactionError::new(
                kind,
                "No model selected for handoff",
            )));
        }
        // The credential lookup is a preflight: a missing key is the exact error, a failed
        // lookup escapes as it is.
        let credential = self
            .provider
            .compaction_credential(&request, progress.cancel.clone())
            .await
            .map_err(|error| CoordinatorError::Compaction(error.into()))?;
        if credential.is_none() {
            return Err(CoordinatorError::Compaction(CompactionError::new(
                kind,
                format!(
                    "No API key for {}",
                    request.provider_id.as_deref().unwrap_or_default()
                ),
            )));
        }
        request.messages.push(CompletionMessage::text(
            MessageRole::User,
            prompts::handoff_instruction(focus),
        ));
        request.tool_choice = Some(ToolChoice::None);
        let caps = self
            .turn
            .target
            .as_ref()
            .map(|t| t.resolution.capabilities.reasoning_efforts.clone())
            .unwrap_or_default();
        request.reasoning_effort = request::resolve_effort(
            self.turn.settings.reasoning_effort.as_deref(),
            &caps,
            !caps.is_empty(),
        );
        let side_identity = self.side_context();
        request.context.session_id = side_identity.session_id;
        request.context.cache_key = side_identity.cache_key;
        request.context.main_turn = false;
        request.context.initiator = ProviderRequestInitiator::Agent;
        let side = match self
            .side(request.clone(), kind, progress, RetryMode::Default, true)
            .await
        {
            Err(error) if tool_choice_rejected(&error) => {
                request.tool_choice = Some(ToolChoice::Auto);
                self.side(request, kind, progress, RetryMode::Default, true)
                    .await
            }
            other => other,
        }
        .map_err(|error| into_coordinator(error, task))?;
        // A cancel that arrived while the response completed beats accepting or saving it.
        if progress.cancel.is_cancelled() {
            return Err(CoordinatorError::Cancelled(task.to_owned()));
        }
        if side.text.trim().is_empty() {
            return Ok(None);
        }
        let saved_path = if auto_triggered && self.compaction.handoff_save_to_disk {
            self.save_handoff(&side.text).await
        } else {
            None
        };
        Ok(Some(HandoffDocument {
            text: side.text,
            usage: side.usage,
            saved_path,
            aborted: side.aborted,
        }))
    }

    /// Saves a handoff document as `handoff-<UTC time>.md` in the session artifacts
    /// directory: the redacted document plus one newline. A persisted session gets its
    /// private artifacts directory on demand (it is otherwise created by the first
    /// artifact); an unpersisted session skips quietly and a failed write warns. Neither
    /// ever fails the handoff.
    async fn save_handoff(&self, document: &str) -> Option<String> {
        let body = format!("{document}\n");
        let saved = self
            .handle
            .call(move |s| {
                let Ok(info) = s.info() else {
                    return Ok(None);
                };
                let directory = info.artifacts_dir.clone();
                let path: PathBuf = directory.join(format!(
                    "handoff-{}.md",
                    s.clock
                        .system_time_rfc3339_millis()
                        .unwrap_or_default()
                        .replace([':', '.'], "-")
                ));
                let safe = s.redactor.redact_text(&body);
                let shown = path.display().to_string();
                let written = crate::store::create_private_dir(&directory)
                    .and_then(|()| crate::store::write_private_atomic(&path, safe.as_bytes()));
                Ok(Some(
                    written
                        .map(|()| shown.clone())
                        .map_err(|error| (shown, error.to_string())),
                ))
            })
            .await;
        match saved {
            Ok(Some(Ok(path))) => Some(path),
            Ok(Some(Err((path, error)))) => {
                let _ = self
                    .warn(format!(
                        "Failed to save handoff document to {path}: {error}"
                    ))
                    .await;
                None
            }
            Ok(None) | Err(_) => None,
        }
    }
}

/// The 400 rejection of tool choice none: its response words are in the provider message or
/// the preserved response body.
fn tool_choice_rejected(error: &CompactionError) -> bool {
    let text = format!(
        "{} {}",
        error.message,
        error.cause.as_deref().unwrap_or_default()
    );
    request::handoff_tool_choice_fallback(error.status, &text)
}
