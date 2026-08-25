use harness_providers::ProviderRequestCostError;
use serde_json::{json, Value};

use super::AttachmentMetadata;

const ATTACHMENTS_PREFIX: &str = "\n<harness-attachments-v1>";
const ATTACHMENTS_SUFFIX: &str = "</harness-attachments-v1>";

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
