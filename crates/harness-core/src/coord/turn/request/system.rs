use super::*;
use harness_providers::{CompletionMessage, MessageContentPart};
use std::collections::BTreeMap;

type Attachments = BTreeMap<usize, Vec<crate::attachment_transport::AttachmentMetadata>>;
type MessageContent = BTreeMap<usize, Vec<MessageContentPart>>;

impl Worker {
    /// Inserts the skill catalog metadata after the leading system messages and the
    /// preloaded skill bodies before everything, shifting the attachment and ordered
    /// content indices that follow each insertion.
    pub(super) fn insert_skill_system(
        &self,
        messages: &mut Vec<CompletionMessage>,
        attachments: &mut Attachments,
        content: &mut MessageContent,
    ) {
        if let Some(metadata) = &self.skill_metadata {
            let after_system = messages
                .iter()
                .take_while(|message| message.role == MessageRole::System)
                .count();
            messages.insert(
                after_system,
                CompletionMessage::text(MessageRole::System, metadata),
            );
            shift_from(attachments, content, after_system);
        }
        if let Some(preloads) = &self.skill_preloads
            && !preloads.is_empty()
        {
            messages.insert(
                0,
                CompletionMessage::text(
                    MessageRole::System,
                    preloads
                        .iter()
                        .map(|(_, body)| body.as_str())
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                ),
            );
            shift_from(attachments, content, 0);
        }
    }

    /// The system text a main request sends: preloaded skill bodies, the agent prompt, then
    /// the skill catalog metadata. Side requests inherit it.
    pub(in crate::coord) fn effective_system(
        &self,
        context: &super::super::super::context::Context,
    ) -> String {
        let mut messages: Vec<CompletionMessage> = context
            .entries
            .iter()
            .take_while(|entry| entry.message.role == MessageRole::System)
            .map(|entry| entry.message.clone())
            .collect();
        self.insert_skill_system(
            &mut messages,
            &mut Attachments::new(),
            &mut MessageContent::new(),
        );
        messages
            .iter()
            .take_while(|message| message.role == MessageRole::System)
            .map(|message| message.content.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

fn shift_from(attachments: &mut Attachments, content: &mut MessageContent, from: usize) {
    *attachments = std::mem::take(attachments)
        .into_iter()
        .map(|(index, media)| (index + usize::from(index >= from), media))
        .collect();
    *content = std::mem::take(content)
        .into_iter()
        .map(|(index, parts)| (index + usize::from(index >= from), parts))
        .collect();
}
