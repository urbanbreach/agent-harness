//! Deployment-aware image and byte allowances for archive frames.
use super::*;
use harness_providers::attachment_protocol::{image_limits, ImageLimits};

/// Images and base64 bytes the request already carries outside the archive.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Payload {
    pub images: u32,
    pub bytes: u64,
}

pub(super) fn retained_payload<'a>(
    attachments: impl Iterator<Item = &'a AttachmentMetadata>,
) -> Payload {
    attachments.fold(Payload::default(), |payload, attachment| Payload {
        images: payload
            .images
            .saturating_add(u32::from(attachment.mime.starts_with("image/"))),
        bytes: payload
            .bytes
            .saturating_add(attachment.size.div_ceil(3).saturating_mul(4)),
    })
}

/// What archive frames may still add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Budgets {
    pub images: u32,
    pub bytes: u64,
}

/// The deployment's image allowance and archive byte budget less the retained payload.
pub(super) fn budgets(limits: &ImageLimits, retained: &Payload) -> Budgets {
    Budgets {
        images: limits.total_images.saturating_sub(retained.images),
        bytes: limits
            .archive_bytes
            .min(limits.per_request_bytes.saturating_sub(retained.bytes)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowances_shrink_by_the_retained_payload() {
        let image = AttachmentMetadata::new("a", "image/png", 3_000_000, None, "ref");
        let retained = retained_payload([&image, &image].into_iter());
        assert_eq!(
            retained,
            Payload {
                images: 2,
                bytes: 8_000_000
            }
        );
        for (provider, model, images, bytes) in [
            ("anthropic", "claude-opus-5", 598, 12_000_000),
            ("openai", "gpt-5.5", 198, 3_000_000),
            ("unlisted", "m", 3, 3_000_000),
        ] {
            let budget = budgets(&image_limits(provider, model, None), &retained);
            assert_eq!((budget.images, budget.bytes), (images, bytes), "{provider}");
        }
    }
}
