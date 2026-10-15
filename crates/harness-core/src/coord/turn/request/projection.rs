//! Archive summary projection of the outgoing request: ordered text and frames.
use super::*;
use crate::coord::compaction::archive::reader::render_reader_view;
use crate::coord::context::archive::{
    archive_projection, compaction_seq, needs_reader_view, ArchiveText, FrameLimits,
};
use crate::coord::context::Context;
use harness_providers::{attachment_protocol::image_limits, CompletionMessage, MessageContentPart};
use std::collections::BTreeMap;

type Attachments = BTreeMap<usize, Vec<crate::attachment_transport::AttachmentMetadata>>;
type Content = BTreeMap<usize, Vec<MessageContentPart>>;

impl Worker {
    fn deployment_probe(
        &self,
        provider: &str,
        model: &str,
    ) -> harness_providers::CompletionRequest {
        harness_providers::CompletionRequest {
            provider_id: Some(provider.to_owned()),
            model_id: model.to_owned(),
            ..Default::default()
        }
    }

    /// Image caps of the deployment the provider will really reach (its endpoint host).
    pub(in crate::coord) fn image_limits_for(
        &self,
        provider: &str,
        model: &str,
    ) -> harness_providers::attachment_protocol::ImageLimits {
        let host = self
            .provider
            .endpoint_host(&self.deployment_probe(provider, model));
        image_limits(provider, model, host.as_deref())
    }

    /// Wire facts the provider reports for the reader, used to resolve archive geometry.
    pub(in crate::coord) fn shape_host_for(
        &self,
        provider: &str,
        model: &str,
    ) -> harness_compaction::archive::ShapeHost {
        harness_compaction::archive::ShapeHost {
            api: self
                .provider
                .token_reader(&self.deployment_probe(provider, model))
                .map(|reader| reader.api),
        }
    }

    /// The archive shape for a reader: configured variant or the model's own.
    pub(in crate::coord) fn shape_for(
        &self,
        provider: &str,
        model: &str,
    ) -> &'static harness_compaction::archive::ArchiveShape {
        let explicit = Some(self.compaction.archive.shape.as_str()).filter(|s| *s != "auto");
        harness_compaction::archive::resolve_shape_on(
            model,
            provider,
            explicit,
            self.shape_host_for(provider, model),
        )
    }

    /// Replaces every archive summary entry with its ordered text and frame parts. Frames
    /// are selected newest-first under the deployment's archive byte budget and the images
    /// the rest of the request leaves free, and only selected frames are loaded. A reader
    /// that excludes archived reasoning sees the cached reasoning-free view of a
    /// reasoning-bearing archive, or a local render when none is ready; this only reads
    /// the caches.
    pub(super) fn project_archives(
        &self,
        messages: &Context,
        model: &crate::agent::AgentModelRef,
        request_messages: &mut [CompletionMessage],
        attachments: &mut Attachments,
        content: &mut Content,
    ) {
        let limits = self.image_limits_for(&model.provider_id, &model.model_id);
        let archives: Vec<usize> = messages
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.compaction.as_ref().is_some_and(|s| s.archive.is_some()))
            .map(|(index, _)| index)
            .collect();
        let used = attachments
            .iter()
            .filter(|(index, _)| !archives.contains(index))
            .flat_map(|(_, list)| list)
            .filter(|a| a.mime.starts_with("image/"))
            .count();
        let mut remaining = limits
            .total_images
            .saturating_sub(u32::try_from(used).unwrap_or(u32::MAX));
        let (policy, shape, key) = self.reader_identity(model);
        let source = messages.frame_source.as_deref();
        // The newest archive claims image room first.
        for index in archives.into_iter().rev() {
            let entry = &messages.entries[index];
            let Some(state) = &entry.compaction else {
                continue;
            };
            let Some(record) = &state.archive else {
                continue;
            };
            let view = needs_reader_view(state, policy).then(|| {
                let seq = compaction_seq(entry, state);
                let cached = (seq != 0)
                    .then(|| messages.render_caches.reader_views().ready(seq, &key))
                    .flatten();
                cached.unwrap_or_else(|| Arc::new(render_reader_view(state, shape)))
            });
            let (guide, record, frames) = match &view {
                Some(view) => (view.summary.as_str(), &view.archive, view.frames.as_slice()),
                None => (state.summary.as_str(), record, entry.attachments.as_slice()),
            };
            let projection = archive_projection(
                &ArchiveText {
                    guide,
                    head: &record.text_head,
                    tail: &record.text_tail,
                    truncated: record.truncated_chars,
                },
                frames,
                record.frames.len(),
                source,
                FrameLimits::provider(record, limits.archive_bytes, remaining),
            );
            remaining = remaining
                .saturating_sub(u32::try_from(projection.attachments.len()).unwrap_or(u32::MAX));
            request_messages[index].content = projection.text;
            content.insert(index, projection.parts);
            if projection.attachments.is_empty() {
                attachments.remove(&index);
            } else {
                attachments.insert(index, projection.attachments);
            }
        }
    }
}
