use harness_providers::ProviderRequestCostError;
use serde_json::{json, Value};

use std::collections::BTreeSet;

use super::AttachmentMetadata;

const ATTACHMENTS_PREFIX: &str = "\n<harness-attachments-v1>";
const ATTACHMENTS_SUFFIX: &str = "</harness-attachments-v1>";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProviderAttachmentMetadataError {
    #[error("attachment id is empty")]
    EmptyId,
    #[error("attachment id appears more than once: {id}")]
    DuplicateId { id: String },
    #[error("attachment `{id}` has invalid MIME metadata")]
    InvalidMime { id: String },
    #[error("attachment `{id}` has invalid dimensions")]
    InvalidDimensions { id: String },
    #[error("attachment `{id}` has an invalid redacted content reference")]
    InvalidContentRef { id: String },
}

pub(crate) fn validate_provider_attachments(
    attachments: &[AttachmentMetadata],
) -> Result<(), ProviderAttachmentMetadataError> {
    let mut ids = BTreeSet::new();
    for attachment in attachments {
        if attachment.id.trim().is_empty() {
            return Err(ProviderAttachmentMetadataError::EmptyId);
        }
        if !ids.insert(attachment.id.clone()) {
            return Err(ProviderAttachmentMetadataError::DuplicateId {
                id: attachment.id.clone(),
            });
        }
        let Some((media_type, media_subtype)) = attachment.mime.split_once('/') else {
            return Err(ProviderAttachmentMetadataError::InvalidMime {
                id: attachment.id.clone(),
            });
        };
        if media_type.is_empty()
            || media_subtype.is_empty()
            || attachment
                .mime
                .bytes()
                .any(|byte| byte.is_ascii_whitespace())
        {
            return Err(ProviderAttachmentMetadataError::InvalidMime {
                id: attachment.id.clone(),
            });
        }
        if attachment
            .dimensions
            .is_some_and(|dimensions| dimensions.width == 0 || dimensions.height == 0)
        {
            return Err(ProviderAttachmentMetadataError::InvalidDimensions {
                id: attachment.id.clone(),
            });
        }
        if !valid_redacted_content_ref(attachment.content_ref.as_str()) {
            return Err(ProviderAttachmentMetadataError::InvalidContentRef {
                id: attachment.id.clone(),
            });
        }
    }
    Ok(())
}

fn valid_redacted_content_ref(value: &str) -> bool {
    let Some(value) = value.strip_prefix("attachment:v1:path-") else {
        return false;
    };
    let Some((path_hash, content_hash)) = value.split_once(":content-") else {
        return false;
    };
    valid_hash(path_hash) && valid_hash(content_hash)
}

fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn lower_provider_attachments(text: &str, attachments: &[AttachmentMetadata]) -> String {
    if attachments.is_empty() {
        return text.to_string();
    }
    let values = attachments
        .iter()
        .map(|attachment| {
            json!({
                "id": attachment.id,
                "mime": attachment.mime,
                "size": attachment.size,
                "dimensions": attachment.dimensions,
                "content_ref": attachment.content_ref.as_str(),
            })
        })
        .collect::<Vec<Value>>();
    format!(
        "{text}{ATTACHMENTS_PREFIX}{}{ATTACHMENTS_SUFFIX}",
        json!({ "attachments": values })
    )
}

pub(crate) fn historical_attachment_tokens<'a>(
    mut attachments: impl Iterator<Item = &'a AttachmentMetadata>,
) -> Result<u32, ProviderRequestCostError> {
    let bytes = attachments.try_fold(0_u64, |total, attachment| {
        total
            .checked_add(attachment.size)
            .ok_or(ProviderRequestCostError::ArithmeticOverflow)
    })?;
    u32::try_from(bytes.div_ceil(4)).map_err(|_| ProviderRequestCostError::ArithmeticOverflow)
}
