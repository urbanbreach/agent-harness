use super::*;

impl SessionProjection {
    pub(super) fn rebuild_compaction_presentation(
        &mut self,
        checkpoints: &[harness_core::transcript_projection::CompactionCheckpointProjection],
    ) {
        self.compaction_status = None;
        self.compaction_usage_metrics = CompactionUsageMetrics::default();
        let Some(checkpoint) = checkpoints.last() else {
            return;
        };
        let (state, label) = match checkpoint.status {
            CompactionCheckpointStatus::Requested => (CompactionState::Requested, "requested"),
            CompactionCheckpointStatus::Written => (CompactionState::Written, "written"),
            CompactionCheckpointStatus::Failed => (CompactionState::Failed, "failed"),
            CompactionCheckpointStatus::Applied
            | CompactionCheckpointStatus::SessionCompacted
            | CompactionCheckpointStatus::BranchSummary => (CompactionState::Applied, "applied"),
        };
        if state == CompactionState::Applied {
            self.compaction_usage_metrics.completed_count = 1;
            self.compaction_usage_metrics.summary_tokens_estimate =
                u64::from(checkpoint.summary_tokens_estimate.unwrap_or(0));
            self.compaction_usage_metrics.reduction_tokens_estimate =
                u64::from(checkpoint.reduction_tokens_estimate.unwrap_or(0));
        }
        self.compaction_usage_metrics.last_tokens_before_estimate = checkpoint
            .tokens_before_estimate
            .or(checkpoint.tokens_before);
        self.compaction_usage_metrics.last_tokens_after_estimate = checkpoint.tokens_after_estimate;
        self.compaction_usage_metrics
            .last_reduction_percent_estimate = checkpoint.reduction_percent_estimate;
        if state == CompactionState::Applied {
            self.active_context_usage = Some(
                checkpoint
                    .tokens_after_estimate
                    .map(ActiveContextUsage::estimate)
                    .unwrap_or_else(ActiveContextUsage::compacted_pending_refresh),
            );
        }
        self.compaction_status = Some(CompactionStatus {
            agent_id: checkpoint.agent_id.clone(),
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            trigger_reason: checkpoint
                .trigger_reason
                .clone()
                .unwrap_or_else(|| label.to_string()),
            state,
            message: format!("compaction {label}"),
        });
    }

    pub(super) fn rebuild_legacy_compaction_presentation(
        &mut self,
        compaction: Option<&CanonicalLegacyCompaction>,
    ) {
        let Some(compaction) = compaction else {
            return;
        };
        let (state, label) = match compaction.status {
            CanonicalLegacyCompactionStatus::Requested => (CompactionState::Requested, "requested"),
            CanonicalLegacyCompactionStatus::Written => (CompactionState::Written, "written"),
            CanonicalLegacyCompactionStatus::Applied => (CompactionState::Applied, "applied"),
            CanonicalLegacyCompactionStatus::Failed => (CompactionState::Failed, "failed"),
        };
        if state == CompactionState::Applied {
            self.active_context_usage = Some(ActiveContextUsage::compacted_pending_refresh());
        }
        let source_label = if compaction.deterministic_fallback {
            " · deterministic fallback"
        } else {
            ""
        };
        self.compaction_status = Some(CompactionStatus {
            agent_id: compaction.agent_id.clone(),
            checkpoint_id: compaction.checkpoint_id.clone(),
            trigger_reason: compaction.trigger_reason.clone(),
            state,
            message: format!("compaction {label}{source_label} · legacy compatibility"),
        });
    }
}
