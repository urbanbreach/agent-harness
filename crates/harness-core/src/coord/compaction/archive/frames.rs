//! Archive frame publication, recovery rendering and attachment materialization.
use super::payload::{budgets, retained_payload};
use super::*;

impl Worker {
    pub(super) async fn publish_frames(
        &self,
        output: &archive::ArchiveOutput,
        progress: &Progress,
    ) -> Result<(), CoordinatorError> {
        let total = u32::try_from(output.frames.len()).unwrap_or(u32::MAX);
        for (index, frame) in output.frames.iter().enumerate() {
            let thumb = archive::thumbnail(frame, THUMB_COLS, THUMB_ROWS);
            progress
                .thumbnail(
                    FrameThumbnail {
                        width: thumb.width,
                        height: thumb.height,
                        bits: base64::engine::general_purpose::STANDARD.encode(&thumb.bits),
                    },
                    u32::try_from(index + 1).unwrap_or(u32::MAX),
                    total,
                )
                .await?;
        }
        Ok(())
    }

    /// Dead-end rescue for an archive summary: re-render its recoverable source at the frame
    /// budget that fits the recovery band and the provider's remaining image allowance.
    /// `context` is the context the summary sits in; its other images count against the
    /// allowance. Returns the replacement state and frames only when the rebuild has
    /// strictly fewer frames; the caller reports the recovery.
    pub(in crate::coord::compaction) async fn rerender_archive(
        &self,
        context: &Context,
        state: &CompactionState,
        tail_tokens: u32,
        measure: &Measure,
        progress: &Progress,
    ) -> Result<Option<(CompactionState, Vec<AttachmentMetadata>)>, CoordinatorError> {
        let target = self.turn.target.as_ref();
        let vision = target.is_some_and(|t| t.resolution.capabilities.supports_vision);
        let provider = target.map_or("", |t| t.provider.as_str());
        let model = target.map_or("", |t| t.model.as_str());
        let exclude = excludes_archived_reasoning(provider, model);
        // Eligibility needs recoverable source: whole text, else the validated text edges.
        let Some(record) = state
            .archive
            .as_ref()
            .filter(|r| vision && !archive_source(r, exclude).is_empty())
        else {
            return Ok(None);
        };
        let existing = record.frames.len();
        if existing <= 1 || progress.cancel.is_cancelled() {
            return Ok(None);
        }
        let shape = *self.shape_for(provider, model);
        let others = context
            .entries
            .iter()
            .filter(|entry| entry.compaction.is_none())
            .flat_map(|entry| &entry.attachments);
        let budget = budgets(
            &self.image_limits_for(provider, model),
            &retained_payload(others),
        );
        let frames = archive::rescue_frame_budget(
            &shape,
            (measure.window > 0).then_some(measure.threshold),
            measure.fixed,
            tail_tokens,
            budget.bytes,
            budget.images,
        );
        if frames == 0 || usize::try_from(frames).unwrap_or(usize::MAX) >= existing {
            return Ok(None);
        }
        let input = ArchivePlanInput {
            previous: Some(ArchiveSource {
                text: archive_source(record, exclude),
                truncated_chars: record.truncated_chars,
            }),
            new_text: String::new(),
            frame_budget: frames,
            shape,
            max_frames: frames,
        };
        let Ok(Ok(output)) =
            tokio::task::spawn_blocking(move || archive::build_archive(&input)).await
        else {
            return Ok(None);
        };
        if progress.cancel.is_cancelled()
            || output.frames.len() >= existing
            || archive_bytes(&output) > budget.bytes
        {
            return Ok(None);
        }
        self.publish_frames(&output, progress).await?;
        let mut ops = harness_compaction::files::FileOps::default();
        harness_compaction::files::merge_previous(
            &mut ops,
            &state.read_files,
            &state.modified_files,
        );
        let (frames, record) = materialize(&output, &shape);
        Ok(Some((
            CompactionState {
                summary: archive::lead_in_with_files(&output, &ops),
                short_summary: Some(output.short_summary.clone()),
                archive: Some(record),
                ..state.clone()
            },
            frames,
        )))
    }
}

/// Frame attachments for the live context and the durable record (paths are filled when
/// the commit persists the frames). `chars` counts text edges as well as framed text.
pub(in crate::coord) fn materialize(
    output: &archive::ArchiveOutput,
    shape: &archive::ArchiveShape,
) -> (Vec<AttachmentMetadata>, ArchiveRecord) {
    let frames = output
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| {
            let mut attachment = AttachmentMetadata::from_bytes(
                format!("archive-frame-{}", index + 1),
                "image/png",
                None,
                &frame.png,
                Some(
                    harness_providers::attachment_protocol::AttachmentDimensions {
                        width: frame.width,
                        height: frame.height,
                    },
                ),
            );
            attachment.detail = output
                .frame_meta
                .get(index)
                .and_then(|meta| meta.detail)
                .map(str::to_owned);
            attachment
        })
        .collect();
    let record = ArchiveRecord {
        text: output.source.text.clone(),
        text_head: output.text_head.clone(),
        text_tail: output.text_tail.clone(),
        truncated_chars: output.source.truncated_chars,
        shape: shape.id.to_owned(),
        chars: output.total_chars,
        frames: output
            .frames
            .iter()
            .zip(&output.frame_meta)
            .map(|(frame, meta)| ArchiveFrameRecord {
                path: String::new(),
                digest: String::new(),
                bytes: frame.png.len() as u64,
                width: frame.width,
                height: frame.height,
                chars: frame.chars,
                columns: meta.columns,
                rows: meta.rows,
                shape: Some(meta.shape.to_owned()),
                detail: meta.detail.map(str::to_owned),
                variant: Some(meta.variant.to_owned()),
            })
            .collect(),
    };
    (frames, record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_keep_per_frame_shape_and_total_chars() -> Result<(), archive::ArchiveError> {
        let shape =
            archive::resolve_shape("gpt-5.5", "openai", Some("8on16-bw")).with_frame_size(96);
        let output = archive::build_archive(&ArchivePlanInput {
            previous: None,
            new_text: "word ".repeat(600),
            frame_budget: 4,
            shape,
            max_frames: 4,
        })?;
        let (frames, record) = materialize(&output, &shape);
        assert!(frames.len() > 1);
        assert_eq!(record.frames.len(), frames.len());
        assert_eq!(record.chars, output.total_chars);
        assert!(record.chars > record.frames.iter().map(|f| f.chars).sum::<u64>());
        for (frame, attachment) in record.frames.iter().zip(&frames) {
            assert_eq!(frame.variant.as_deref(), Some("bw"));
            assert!(frame.columns > 0 && frame.rows > 0);
            assert_eq!(attachment.detail.as_deref(), Some("original"));
        }
        Ok(())
    }
}
