//! The archive method: older history rendered into dense bitmap frames for vision models.
use super::super::{
    context::{CompactionState, Context},
    turn::Worker,
    *,
};
use super::commit::Draft;
use super::maintain::{Measure, Reason, Request};
use super::progress::Progress;
use crate::attachment_transport::AttachmentMetadata;
use crate::event::{ArchiveFrameRecord, ArchiveRecord, CompactionPhase, FrameThumbnail};
use base64::Engine as _;
use harness_compaction::{
    archive::{self, ArchivePlanInput, ArchiveSource, FrameBudgetInput, SerializeOptions},
    prepare::Preparation,
    CompactionMethod,
};

mod frames;
mod payload;
pub(in crate::coord) mod reader;
mod source;
use frames::materialize;
use payload::{budgets, retained_payload};
use source::earlier_history_digest;
pub(in crate::coord) use source::{
    archive_previous_summary, archive_source, excludes_archived_reasoning,
};

const THUMB_COLS: u16 = 6;
const THUMB_ROWS: u16 = 3;

/// Why the archive method cannot run: the line an automatic pass reports as a warning and
/// the error a manual pass fails with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord::compaction) struct ArchiveBlocked {
    pub warning: String,
    pub error: String,
}

impl ArchiveBlocked {
    fn new(warning: &str, error: &str) -> Self {
        Self {
            warning: warning.to_owned(),
            error: error.to_owned(),
        }
    }

    /// An automatic-only block: manual passes surface the same line as their error.
    fn same(text: &str) -> Self {
        Self::new(text, text)
    }
}

type Drafted = Result<Draft, ArchiveBlocked>;

impl Worker {
    /// Builds an archive draft, or explains why the archive cannot run.
    pub(super) async fn archive_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        request: &Request,
        measure: &Measure,
        progress: &Progress,
    ) -> Result<Drafted, CoordinatorError> {
        progress
            .phase(Some(CompactionMethod::Archive), CompactionPhase::Archiving)
            .await?;
        let target = self.turn.target.clone();
        let provider = target
            .as_ref()
            .map_or("", |t| t.provider.as_str())
            .to_owned();
        let model_id = target.as_ref().map_or("", |t| t.model.as_str()).to_owned();
        let explicit = Some(self.compaction.archive.shape.as_str()).filter(|s| *s != "auto");
        let shape = *self.shape_for(&provider, &model_id);
        let items = context.items_with(&self.counter);
        let discarded = prep.history.start..prep.turn_prefix.end.max(prep.history.end);
        let exclude_reasoning = excludes_archived_reasoning(&provider, &model_id);
        let mut new_text = archive::serialize_for_archive(
            &items[discarded],
            &SerializeOptions {
                include_thinking: !exclude_reasoning,
                ..SerializeOptions::default()
            },
        );
        let retained = context
            .entries
            .iter()
            .take(body)
            .chain(context.entries.iter().skip(body + prep.recent.start))
            .flat_map(|entry| &entry.attachments);
        let budget = budgets(
            &self.image_limits_for(&provider, &model_id),
            &retained_payload(retained),
        );
        let sum = |range: std::ops::Range<usize>, floor: bool| {
            items
                .get(range)
                .unwrap_or_default()
                .iter()
                .fold(0u32, |n, item| {
                    n.saturating_add(if floor {
                        item.tokens_floor
                    } else {
                        item.tokens
                    })
                })
        };
        let kept = sum(prep.recent.clone(), false);
        // Opaque (encrypted) reasoning does not count toward the threshold target.
        let kept_plain = sum(prep.recent.clone(), true);
        let pending = sum(prep.recent.end..items.len(), false);
        let frame_budget = archive::plan_frame_budget(&FrameBudgetInput {
            window: measure.window,
            reserve: self.compaction.reserve_tokens,
            nonmessage_tokens: measure.fixed,
            kept_tokens: kept,
            kept_without_reasoning_tokens: kept_plain,
            pending_tokens: pending,
            threshold: if measure.window > 0 {
                measure.threshold
            } else {
                u32::MAX
            },
            shape,
            byte_budget: budget.bytes,
            image_budget: budget.images,
        });
        if frame_budget == 0 {
            return Ok(Err(if budget.images == 0 || budget.bytes == 0 {
                ArchiveBlocked::new(
                    "archive produced too much standing image payload.",
                    "archive cannot run locally: standing image payload exceeds the per-request budget.",
                )
            } else {
                ArchiveBlocked::new(
                    "archive: kept history alone exceeds the context budget.",
                    "archive cannot run locally: kept history alone exceeds the context budget.",
                )
            }));
        }
        let previous_state = context
            .entries
            .get(body)
            .and_then(|e| e.compaction.as_ref());
        let previous = previous_state
            .and_then(|state| state.archive.as_ref())
            .map(|record| ArchiveSource {
                text: archive_source(record, exclude_reasoning),
                truncated_chars: record.truncated_chars,
            })
            .filter(|source| !source.text.is_empty());
        if previous.is_none()
            && let Some(state) = previous_state
        {
            new_text = earlier_history_digest(&state.summary, &new_text);
        }
        // The rendering probe is the carried source (or the earlier-summary digest already in
        // `new_text`) joined to the fresh history by a FULL BLOCK; it may promote the shape to
        // the Silver fallback. Budget planning above keeps the model's own geometry.
        let coverage_text = match previous.as_ref().map(|source| source.text.as_str()) {
            Some(source) if !new_text.is_empty() => format!("{source}\u{2588}{new_text}"),
            Some(source) => source.to_owned(),
            None => new_text.clone(),
        };
        let shape = *archive::resolve_shape_for_text_on(
            &model_id,
            &provider,
            explicit,
            &coverage_text,
            self.shape_host_for(&provider, &model_id),
        );
        let coverage = archive::font_coverage(&coverage_text, shape.font);
        if !coverage.is_safe() {
            let ratio = coverage.unsupported_ratio() * 100.0;
            return Ok(Err(ArchiveBlocked::new(
                &format!(
                    "archive disabled: unsupported characters for selected archive font ({ratio:.1}%)."
                ),
                &format!(
                    "archive cannot render this conversation locally: unsupported characters for selected archive font ({ratio:.1}%)."
                ),
            )));
        }
        let mut cap = frame_budget;
        for attempt in 0..=2 {
            let output = render_archive(ArchivePlanInput {
                previous: previous.clone(),
                new_text: new_text.clone(),
                frame_budget: cap,
                shape,
                max_frames: cap,
            })
            .await?;
            let count = u32::try_from(output.frames.len()).unwrap_or(u32::MAX);
            let bytes = archive_bytes(&output);
            let frame_tokens = output.frames.iter().fold(0u32, |n, frame| {
                n.saturating_add(self.counter.frame_tokens(frame.width, frame.height))
            });
            let text_tokens = self
                .counter
                .count_text(&archive::lead_in_with_files(&output, &prep.file_ops))
                .saturating_add(self.counter.count_text(&output.text_head))
                .saturating_add(self.counter.count_text(&output.text_tail));
            let projection = measure
                .fixed
                .saturating_add(kept_plain)
                .saturating_add(pending)
                .saturating_add(text_tokens)
                .saturating_add(frame_tokens);
            let mut next = count;
            if bytes > budget.bytes {
                next = u32::try_from(u64::from(count).saturating_mul(budget.bytes) / bytes.max(1))
                    .unwrap_or(0);
            }
            let target = measure.threshold.saturating_mul(3) / 5;
            if measure.window > 0 && projection > target && count > 1 {
                let mean = frame_tokens.checked_div(count).unwrap_or(0).max(1);
                next = next.min(
                    count
                        .saturating_sub(projection.saturating_sub(target).div_ceil(mean))
                        .max(1),
                );
            }
            if attempt == 2 || next == 0 || next >= count {
                if bytes > budget.bytes || count > budget.images {
                    return Ok(Err(ArchiveBlocked::new(
                        "archive produced too much standing image payload.",
                        "archive cannot run locally: standing image payload exceeds the per-request budget.",
                    )));
                }
                return self
                    .finish_archive_draft(
                        context, body, prep, request, measure, progress, output, &shape,
                    )
                    .await;
            }
            cap = next;
        }
        Ok(Err(ArchiveBlocked::same(
            "archive could not bring the context under the limit.",
        )))
    }
}

impl Worker {
    #[expect(
        clippy::too_many_arguments,
        reason = "Archive acceptance uses preparation and live budget"
    )]
    async fn finish_archive_draft(
        &self,
        context: &Context,
        body: usize,
        prep: &Preparation,
        request: &Request,
        measure: &Measure,
        progress: &Progress,
        output: archive::ArchiveOutput,
        shape: &archive::ArchiveShape,
    ) -> Result<Drafted, CoordinatorError> {
        let (frames, record) = materialize(&output, shape);
        let mut draft = self.text_draft(
            context,
            body,
            prep,
            String::new(),
            Some(output.short_summary.clone()),
            CompactionMethod::Archive,
            measure,
            None,
            None,
        );
        draft.state.summary = archive::lead_in_with_files(&output, &prep.file_ops);
        draft.state.archive = Some(record);
        draft.frames = frames;
        let projected = self.measure(&self.replacement(context, &draft))?;
        if !projected.fits() {
            return Ok(Err(ArchiveBlocked::new(
                "archive could not bring the context under the limit.",
                "archive could not bring the context under the limit locally.",
            )));
        }
        if projected.tokens >= measure.tokens {
            return Ok(Err(ArchiveBlocked::new(
                "archive would not reduce context.",
                "archive would not reduce context locally.",
            )));
        }
        if request.reason == Reason::Threshold && !projected.has_headroom() {
            let later = request
                .order
                .iter()
                .skip_while(|m| **m != CompactionMethod::Archive)
                .skip(1)
                .any(|m| {
                    matches!(
                        m,
                        CompactionMethod::Soft
                            | CompactionMethod::Remote
                            | CompactionMethod::Handoff
                    ) && self.method_available(*m, request).is_ok()
                });
            if later {
                return Ok(Err(ArchiveBlocked::same(
                    "archive could not leave room under the compaction trigger.",
                )));
            }
            let warning = "archive could not leave room under the compaction trigger; keeping its archive because no later summarizing compaction method is usable.";
            self.warn(warning.to_owned()).await?;
            draft.state.warning = Some(warning.to_owned());
        }
        self.publish_frames(&output, progress).await?;
        Ok(Ok(draft))
    }
}

async fn render_archive(
    input: ArchivePlanInput,
) -> Result<archive::ArchiveOutput, CoordinatorError> {
    tokio::task::spawn_blocking(move || archive::build_archive(&input))
        .await
        .map_err(|_| CoordinatorError::Invalid("archive rendering stopped".into()))?
        .map_err(|error| CoordinatorError::Invalid(error.to_string()))
}

/// Base64 bytes of every frame, the unit provider payload budgets count.
fn archive_bytes(output: &archive::ArchiveOutput) -> u64 {
    output.frames.iter().fold(0u64, |bytes, frame| {
        bytes.saturating_add((frame.png.len() as u64).div_ceil(3).saturating_mul(4))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earlier_summary_survives_archive_planning() -> Result<(), Box<dyn std::error::Error>> {
        let shape = *archive::resolve_shape("", "", None);
        let output = archive::build_archive(&ArchivePlanInput {
            previous: None,
            new_text: earlier_history_digest("Keep the compatibility contract.", "¶user:Continue."),
            frame_budget: 1,
            shape,
            max_frames: 1,
        })?;
        assert!(output
            .source
            .text
            .contains("Keep the compatibility contract."));
        assert!(output.source.text.contains("[Recent conversation]"));
        Ok(())
    }
}
