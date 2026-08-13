use std::time::Duration;

use crate::scheduling::{MotionDemand, MotionPlan};

use super::{ActivityStatus, AppState, ToolCallDisplayStatus, TOOL_FINISH_FLASH_DURATION};

const FAST_CADENCE: Duration = Duration::from_millis(33);
const STREAM_CADENCE: Duration = Duration::from_millis(133);
const STARTUP_CADENCE: Duration = Duration::from_millis(83);
const BACKGROUND_CADENCE: Duration = Duration::from_millis(264);

impl AppState {
    pub(crate) fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
        if reduced_motion {
            self.transcript_view.tool_motion.clear_finish_flashes();
        }
    }

    pub(crate) const fn transcript_motion_enabled(&self) -> bool {
        !self.reduced_motion
    }

    pub(crate) fn refresh_motion_state(&mut self) -> bool {
        let now = self.now();
        let mut changed = self
            .transcript_view
            .tool_motion
            .advance(now, !self.reduced_motion);
        if changed {
            self.bump_transcript_render_epoch();
        }
        self.clear_expired_interrupt_confirmation();
        changed |= self.refresh_toast_motion(now);
        changed
    }

    pub(crate) fn motion_plan(&self) -> MotionPlan {
        let now = self.now();
        let mut plan = MotionPlan::none();

        if let Some(remaining) = self.toast_motion_remaining(now) {
            plan = plan.merge(MotionDemand::until(remaining));
            if !self.reduced_motion && self.toast_requires_fade(now) {
                plan = plan.merge(MotionDemand::fast(FAST_CADENCE));
            }
        }
        if let Some(deadline) = self.interrupt_confirm_deadline {
            plan = plan.merge(MotionDemand::until(deadline.saturating_duration_since(now)));
        }
        if let Some(remaining) = self
            .transcript_view
            .tool_motion
            .earliest_finish_flash_remaining(now, TOOL_FINISH_FLASH_DURATION)
        {
            plan = plan.merge(MotionDemand::until(remaining));
            if !self.reduced_motion && self.transcript_view.visible_running_tool_motion.get() {
                plan = plan.merge(MotionDemand::fast(FAST_CADENCE));
            }
        }
        if !self.reduced_motion {
            plan = if self.fast_visible_motion_active() {
                plan.merge(MotionDemand::fast(FAST_CADENCE))
            } else if self.starting_session_seed_visible() {
                plan.merge(MotionDemand::slow(STARTUP_CADENCE))
            } else if self.streaming_wait_motion_active() {
                plan.merge(MotionDemand::slow(STREAM_CADENCE))
            } else if self.active_background_task_count() > 0 {
                plan.merge(MotionDemand::slow(BACKGROUND_CADENCE))
            } else {
                plan
            };
        }
        let visual_sample = plan.cadence().interval().map_or(0, |interval| {
            let elapsed = now.saturating_duration_since(self.motion_epoch_started_at);
            u64::try_from(elapsed.as_nanos() / interval.as_nanos()).unwrap_or(u64::MAX)
        });
        plan.with_revision(self.motion_revision)
            .with_visual_sample(visual_sample)
    }

    pub(crate) fn sample_motion_clock(&mut self) {
        if self.reduced_motion {
            self.transcript_view.transcript_animation_phase = 0;
            return;
        }
        let elapsed = self
            .now()
            .saturating_duration_since(self.motion_epoch_started_at);
        let period = Duration::from_millis(crate::scheduling::active_animation_period_ms());
        self.transcript_view.transcript_animation_phase =
            usize::try_from(elapsed.as_millis() / period.as_millis()).unwrap_or(usize::MAX);
    }

    pub fn motion_plan_for_evidence(&self) -> MotionPlan {
        self.motion_plan()
    }

    pub fn set_starting_motion_for_evidence(&mut self, visible: bool) {
        self.set_starting_session_seed(visible);
        self.motion_epoch_started_at = self.now();
        self.motion_revision = self.motion_revision.wrapping_add(1);
    }

    pub fn set_reduced_motion_for_evidence(&mut self, reduced_motion: bool) {
        self.set_reduced_motion(reduced_motion);
    }

    pub fn refresh_motion_for_evidence(&mut self) -> bool {
        self.refresh_motion_state()
    }

    pub fn advance_wall_clock_for_motion_evidence(&mut self, elapsed: Duration) {
        let now = self.now() + elapsed;
        self.now_fn = std::sync::Arc::new(move || now);
        self.sample_motion_clock();
    }

    pub fn motion_revision_for_evidence(&self) -> u64 {
        self.motion_revision
    }

    pub(crate) fn startup_motion_phase(&self) -> usize {
        let elapsed = self
            .now()
            .saturating_duration_since(self.motion_epoch_started_at);
        let frames = elapsed.as_millis() / STARTUP_CADENCE.as_millis();
        usize::try_from(frames.saturating_mul(4)).unwrap_or(usize::MAX)
    }

    fn fast_visible_motion_active(&self) -> bool {
        self.activities.iter().any(|activity| {
            activity.status == ActivityStatus::Streaming
                && ((!activity.thinking_text.is_empty() && activity.transcript_text.is_empty())
                    || (self.transcript_view.visible_running_tool_motion.get()
                        && activity
                            .tool_calls
                            .iter()
                            .any(|tool| tool.status == ToolCallDisplayStatus::Running)))
        })
    }

    fn streaming_wait_motion_active(&self) -> bool {
        !self.replay_mode
            && !self.interrupt_requested()
            && self.active_permission().is_none()
            && self.activities.iter().any(|activity| {
                activity.status == ActivityStatus::Streaming
                    && !activity.tool_calls.iter().any(|tool| {
                        matches!(
                            tool.status,
                            ToolCallDisplayStatus::Queued
                                | ToolCallDisplayStatus::PendingPermission
                                | ToolCallDisplayStatus::Running
                        )
                    })
            })
    }
}
