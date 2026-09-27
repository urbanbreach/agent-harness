use super::*;
use crate::{attachment_transport::AttachmentMetadata, file_tag::SelectedPromptTags};
use std::path::Path;
mod tags;

#[derive(Default)]
pub(super) struct Prompt {
    pub reserved_id: Option<String>,
    pub text: String,
    pub tags: SelectedPromptTags,
    pub attachments: Vec<AttachmentMetadata>,
}
impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }
}
impl Prompt {
    pub fn validate(&self, redactor: &dyn Redactor) -> Result<(), CoordinatorError> {
        if (self.text.trim().is_empty() && self.attachments.is_empty())
            || self.text.len() > 1024 * 1024
        {
            return Err(CoordinatorError::Invalid(
                "prompt requires text or attachments and must fit within 1 MiB".into(),
            ));
        }
        validate_attachments(&self.attachments, redactor)?;
        tags::validate(&self.text, &self.tags)
    }
}

pub(super) fn validate_attachments(
    attachments: &[AttachmentMetadata],
    redactor: &dyn Redactor,
) -> Result<(), CoordinatorError> {
    use harness_providers::attachment_protocol::{MAX_ATTACHMENTS, MAX_REQUEST_ATTACHMENT_BYTES};
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(CoordinatorError::Invalid("too many attachments".into()));
    }
    let mut size = 0usize;
    let mut ids = std::collections::BTreeSet::new();
    for attachment in attachments {
        let bytes = attachment
            .bytes()
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        size = size.saturating_add(bytes.len());
        let metadata = serde_json::to_string(attachment)?;
        let content = String::from_utf8_lossy(bytes);
        if size > MAX_REQUEST_ATTACHMENT_BYTES
            || attachment.id.is_empty()
            || attachment.id.len() > 128
            || attachment.id.chars().any(char::is_control)
            || !ids.insert(&attachment.id)
            || redactor.redact_text(&metadata) != metadata
            || redactor.redact_text(&content) != content
        {
            return Err(CoordinatorError::Invalid(
                "attachment exceeds its bounds, has a duplicate id, or contains a credential"
                    .into(),
            ));
        }
    }
    Ok(())
}

impl CoordinatorHandle {
    pub async fn request_agent_turn_with_model_and_selected_tags_and_attachments(
        &self,
        actor: EventActor,
        agent: impl Into<String>,
        text: impl Into<String>,
        tags: SelectedPromptTags,
        attachments: Vec<AttachmentMetadata>,
        model: Option<String>,
        settings: Option<AgentModelSettings>,
    ) -> Result<String, CoordinatorError> {
        let (agent, prompt) = (
            agent.into(),
            Prompt {
                reserved_id: None,
                text: text.into(),
                tags,
                attachments,
            },
        );
        self.call(move |s| s.queue_turn(actor, &agent, prompt, model, settings, None))
            .await
    }
    pub async fn request_agent_turn_with_model_target_and_selected_tags_and_attachments(
        &self,
        actor: EventActor,
        agent: impl Into<String>,
        text: impl Into<String>,
        tags: SelectedPromptTags,
        attachments: Vec<AttachmentMetadata>,
        target: ResolvedModelTarget,
    ) -> Result<String, CoordinatorError> {
        let (agent, prompt) = (
            agent.into(),
            Prompt {
                reserved_id: None,
                text: text.into(),
                tags,
                attachments,
            },
        );
        self.call(move |s| {
            s.queue_turn(
                actor,
                &agent,
                prompt,
                Some(target.model_ref.clone()),
                Some((&target).into()),
                Some(target),
            )
        })
        .await
    }
}

impl Runtime {
    pub fn persist_attachments(
        &mut self,
        actor: &EventActor,
        tool: Option<&str>,
        attachments: &[AttachmentMetadata],
    ) -> Result<Vec<crate::tool::ArtifactRef>, CoordinatorError> {
        attachments
            .iter()
            .map(|attachment| {
                self.write_blob(
                    actor,
                    tool,
                    "attachment",
                    attachment
                        .bytes()
                        .map_err(|e| CoordinatorError::Invalid(e.to_string()))?,
                    BTreeMap::new(),
                )
            })
            .collect()
    }
}

fn hydrate(attachments: &mut [AttachmentMetadata], run_dir: &Path) -> Result<(), CoordinatorError> {
    use harness_providers::attachment_protocol::MAX_ATTACHMENT_BYTES;
    for attachment in attachments {
        let digest = attachment
            .content_ref
            .strip_prefix("attachment:blake3:")
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| {
                CoordinatorError::Invalid("attachment has an invalid content reference".into())
            })?;
        let path = run_dir.join(format!("artifacts/{digest}.attachment"));
        let bytes = crate::store::read_private_bytes(&path, MAX_ATTACHMENT_BYTES as u64)?
            .ok_or_else(|| CoordinatorError::Invalid("attachment artifact is missing".into()))?;
        *attachment = attachment
            .clone()
            .with_bytes(bytes)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
    }
    Ok(())
}

fn load_context(event: &ArtifactWrittenEvent, run_dir: &Path) -> Result<String, CoordinatorError> {
    if event.digest.len() != 64
        || !event.digest.bytes().all(|c| c.is_ascii_hexdigit())
        || event.path != format!("artifacts/{}.txt", event.digest)
    {
        return Err(CoordinatorError::Invalid(
            "invalid prompt context artifact".into(),
        ));
    }
    let bytes = crate::store::read_private_bytes(&run_dir.join(&event.path), 1024 * 1024)?
        .ok_or_else(|| CoordinatorError::Invalid("prompt context artifact is missing".into()))?;
    if bytes.len() as u64 != event.bytes || blake3::hash(&bytes).to_hex().as_str() != event.digest {
        return Err(CoordinatorError::Invalid(
            "prompt context artifact digest mismatch".into(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| CoordinatorError::Invalid("prompt context is not UTF-8".into()))
}

pub(super) fn restore_content(
    context: &mut super::context::Context,
    run_dir: &Path,
    contexts: &std::collections::HashMap<String, &ArtifactWrittenEvent>,
) -> Result<(), CoordinatorError> {
    use harness_providers::attachment_protocol::{MAX_ATTACHMENTS, MAX_REQUEST_ATTACHMENT_BYTES};
    let (count, size) = context
        .entries
        .iter()
        .flat_map(|e| &e.attachments)
        .fold((0usize, 0u64), |(count, size), a| {
            (count.saturating_add(1), size.saturating_add(a.size))
        });
    if count > MAX_ATTACHMENTS || size > MAX_REQUEST_ATTACHMENT_BYTES as u64 {
        return Err(CoordinatorError::Invalid(
            "restored attachments exceed the request limit".into(),
        ));
    }
    for entry in &mut context.entries {
        hydrate(&mut entry.attachments, run_dir)?;
        if entry.message.role != harness_providers::MessageRole::User {
            continue;
        }
        if let Some(context) = entry.turn.as_ref().and_then(|id| contexts.get(id)) {
            entry.message.content.push_str(&format!(
                "\n\nSelected context:\n{}",
                load_context(context, run_dir)?
            ));
        }
    }
    Ok(())
}
