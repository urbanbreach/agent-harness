//! Dead-end rescue: elide heavy output, then drop images, until the caller can progress.
use super::*;
use harness_compaction::shake;

impl Worker {
    /// Dead-end rescue on a working context: elide heavy blocks (unless a shake already
    /// ran), then drop images, stopping as soon as `goal` holds. Edits are applied to
    /// `context` but not journaled; the caller journals with [`Worker::emit_rescue`].
    ///
    /// An elision failure other than cancellation is reported and the image tier still
    /// runs. The goal is only re-tested after something was actually reclaimed, so
    /// success never comes from dropping nothing.
    pub(in crate::coord::compaction) async fn rescue(
        &self,
        context: &mut Context,
        goal: RescueGoal,
        progress: &Progress,
        skip_elide: bool,
    ) -> Result<Rewrite, CoordinatorError> {
        let mut rewrite = Rewrite::default();
        if !skip_elide {
            let options = ElideOptions::new(progress.cancel.clone(), progress.generation);
            match self
                .elide(
                    context,
                    &shake::RESCUE,
                    &self.default_protected_matchers(),
                    &options,
                )
                .await
            {
                Ok(elided) => rewrite = elided,
                Err(
                    error @ (CoordinatorError::CompactionCancelled { .. }
                    | CoordinatorError::Cancelled(_)),
                ) => return Err(error),
                Err(error) => {
                    self.warn(format!(
                        "Could not elide heavy output during dead-end recovery: {error}"
                    ))
                    .await?;
                }
            }
        }
        let reached = rewrite.reclaimed()
            && match goal {
                RescueGoal::Fits => self.measure(context)?.fits(),
                RescueGoal::Headroom => self.measure(context)?.has_headroom(),
                RescueGoal::Preparation(pending) => self.prepare(context, pending).is_some(),
            };
        if !reached && !progress.cancel.is_cancelled() {
            let images = self.drop_images(context);
            rewrite.images += images.images;
            rewrite.edits.extend(images.edits);
            rewrite.touched.extend(images.touched);
        }
        Ok(rewrite)
    }
}
