//! Recovery after the provider timed out reading the request body.
use super::*;

/// Shown when the mechanical reduction could not run safely.
const REDUCE_FAILED: &str = "Could not safely reduce this request before retrying it.";

impl Worker {
    /// Shrinks the request body with one artifact-backed, tool-result-only elision, so the
    /// same request can be sent again smaller. Runs only when automatic maintenance is
    /// enabled and `shake` is configured. Returns true when history changed. Cancellation,
    /// or a prompt generation that moved during the work, ends it silently; any other
    /// failure is a warning and ordinary retry handling continues.
    pub(in crate::coord) async fn body_read_timeout_recovery(
        &mut self,
        context: &mut Context,
    ) -> Result<bool, CoordinatorError> {
        if !self.effective_enabled()
            || !self
                .compaction
                .method_order
                .contains(&CompactionMethod::Shake)
            || self.cancellation.is_cancelled()
        {
            return Ok(false);
        }
        let generation = self.prompt_generation().await?;
        let cancel = self.cancellation.clone();
        let reduced = self
            .recover_request_body(context, &cancel, generation)
            .await;
        if self.cancellation.is_cancelled() || self.prompt_generation().await? != generation {
            return Ok(false);
        }
        match reduced {
            Ok(freed) => Ok(freed > 0),
            Err(CoordinatorError::Cancelled(_) | CoordinatorError::CompactionCancelled { .. }) => {
                Ok(false)
            }
            Err(_) => {
                self.warn(REDUCE_FAILED.to_owned()).await?;
                Ok(false)
            }
        }
    }
}
