use std::fmt::{Display, Formatter};

use harness_core::attachment_transport::{
    AttachmentMetadata, AttachmentOrderingError, stable_attachment_order,
};

use crate::attachment_lifecycle::{Attachment, MimeKind};
use crate::completion_controller::{
    CompletionAcceptance, CompletionController, CompletionItem, CompletionRequest,
    CompletionTrigger,
};
use crate::composer_editing::DeleteKind;
use crate::composer_integration::{ComposerSlice, ComposerSliceError};

use super::TargetIdentity;
use super::stale_target::{AttachmentCapability, StaleTargetGuard, TargetSnapshot};

#[derive(Debug)]
pub enum ReplyComposerError {
    Composer(ComposerSliceError),
    AttachmentCapability {
        capability: AttachmentCapability,
        mime: MimeKind,
    },
    AttachmentOrdering(AttachmentOrderingError),
    Disabled,
}

impl Display for ReplyComposerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Composer(error) => Display::fmt(error, formatter),
            Self::AttachmentCapability { capability, mime } => write!(
                formatter,
                "attachment MIME {} is not supported by {capability}",
                mime.as_str()
            ),
            Self::AttachmentOrdering(error) => Display::fmt(error, formatter),
            Self::Disabled => formatter.write_str("dashboard reply composer is disabled"),
        }
    }
}

impl std::error::Error for ReplyComposerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Composer(error) => Some(error),
            Self::AttachmentOrdering(error) => Some(error),
            Self::AttachmentCapability { .. } | Self::Disabled => None,
        }
    }
}

impl From<ComposerSliceError> for ReplyComposerError {
    fn from(error: ComposerSliceError) -> Self {
        Self::Composer(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyAttachment {
    pub id: u64,
    pub metadata: AttachmentMetadata,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyPayload {
    pub text: String,
    pub attachments: Vec<ReplyAttachment>,
}

pub struct DashboardReplyComposer {
    target: TargetIdentity,
    guard: StaleTargetGuard,
    capabilities: AttachmentCapability,
    disabled: bool,
    slice: ComposerSlice,
}

impl DashboardReplyComposer {
    pub fn new(snapshot: &TargetSnapshot, draft: &str) -> Self {
        Self {
            target: snapshot.identity.clone(),
            guard: StaleTargetGuard::from_snapshot(snapshot),
            capabilities: snapshot.capabilities.attachment,
            disabled: snapshot.lifecycle.is_closed() || !snapshot.capabilities.can_reply,
            slice: ComposerSlice::from_text(draft),
        }
    }

    pub fn refresh(&mut self, snapshot: &TargetSnapshot) {
        self.capabilities = snapshot.capabilities.attachment;
        self.disabled = snapshot.lifecycle.is_closed() || !snapshot.capabilities.can_reply;
    }

    pub fn target(&self) -> &TargetIdentity {
        &self.target
    }

    pub const fn guard(&self) -> &StaleTargetGuard {
        &self.guard
    }

    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn text(&self) -> String {
        self.slice.editor().text()
    }

    pub fn insert_text(&mut self, text: &str) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.insert_text(text)?;
        Ok(())
    }

    pub fn paste(&mut self, text: &str) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.paste(text)?;
        Ok(())
    }

    pub fn backspace(&mut self) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.backspace()?;
        Ok(())
    }

    pub fn delete(&mut self, kind: DeleteKind) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.delete(kind)?;
        Ok(())
    }

    pub fn move_left(&mut self) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.move_left()?;
        Ok(())
    }

    pub fn move_right(&mut self) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.move_right()?;
        Ok(())
    }

    pub fn attach(&mut self, id: u64, attachment: Attachment) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        let mime = attachment.mime();
        if !self.capabilities.supports(mime) {
            return Err(ReplyComposerError::AttachmentCapability {
                capability: self.capabilities,
                mime,
            });
        }
        self.slice
            .attach(crate::composer_atoms::AttachmentId::new(id), attachment)?;
        Ok(())
    }

    pub fn begin_completion(
        &mut self,
        trigger: CompletionTrigger,
    ) -> Result<CompletionRequest, ReplyComposerError> {
        self.ensure_enabled()?;
        Ok(self.slice.begin_completion(trigger))
    }

    pub fn apply_completion_results(
        &mut self,
        request: &CompletionRequest,
        results: Vec<CompletionItem>,
    ) -> Result<(), ReplyComposerError> {
        self.ensure_enabled()?;
        self.slice.apply_completion_results(request, results)?;
        Ok(())
    }

    pub fn accept_completion_keyboard(
        &mut self,
    ) -> Result<CompletionAcceptance, ReplyComposerError> {
        self.ensure_enabled()?;
        Ok(self.slice.accept_completion_keyboard()?)
    }

    pub fn completion(&self) -> &CompletionController {
        self.slice.completion()
    }

    pub fn payload(&self) -> Result<ReplyPayload, ReplyComposerError> {
        let metadata = self
            .slice
            .attachments()
            .iter()
            .map(|entry| {
                AttachmentMetadata::from_bytes(
                    entry.id.get().to_string(),
                    entry.attachment.mime().as_str(),
                    entry.attachment.canonical_path(),
                    entry.attachment.bytes(),
                    None,
                )
            })
            .collect::<Vec<_>>();
        let ordered =
            stable_attachment_order(&metadata).map_err(ReplyComposerError::AttachmentOrdering)?;
        let attachments = self
            .slice
            .attachments()
            .iter()
            .zip(ordered)
            .map(|(entry, metadata)| ReplyAttachment {
                id: entry.id.get(),
                metadata,
                bytes: entry.attachment.bytes().to_vec(),
            })
            .collect();
        Ok(ReplyPayload {
            text: self.text(),
            attachments,
        })
    }

    fn ensure_enabled(&self) -> Result<(), ReplyComposerError> {
        if self.disabled {
            Err(ReplyComposerError::Disabled)
        } else {
            Ok(())
        }
    }
}
