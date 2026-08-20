use std::time::Duration;

use crate::scheduling::{MotionDemand, MotionPlan};

use super::{ActivityStatus, AppState, ToolCallDisplayStatus};

const FAST_CADENCE: Duration = Duration::from_millis(33);
const STREAM_CADENCE: Duration = Duration::from_millis(133);
const STARTUP_CADENCE: Duration = Duration::from_millis(83);
const STARTUP_EXPANSION_DELAY: Duration = Duration::from_millis(300);
const BACKGROUND_CADENCE: Duration = Duration::from_millis(264);

impl AppState {
    pub(crate) fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.reduced_motion = reduced_motion;
    }

    pub(crate) const fn transcript_motion_enabled(&self) -> bool {
        !self.reduced_motion
    }

    pub(crate) fn refresh_motion_state(&mut self) -> bool {
        let now = self.now();
        self.clear_expired_interrupt_confirmation();
        self.refresh_toast_motion(now)
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
        if !self.reduced_motion {
            plan = if self.fast_visible_motion_active() {
                plan.merge(MotionDemand::fast(FAST_CADENCE))
            } else if self.startup_welcome_transition_pending()
                || self.starting_session_seed_visible()
            {
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
            self.sampled_motion_elapsed = Duration::ZERO;
            self.transcript_view.transcript_animation_phase = 0;
            return;
        }
        let elapsed = self
            .now()
            .saturating_duration_since(self.motion_epoch_started_at);
        self.sampled_motion_elapsed = elapsed;
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
        self.sampled_motion_elapsed = Duration::ZERO;
        self.motion_revision = self.motion_revision.wrapping_add(1);
    }

    pub fn set_reduced_motion_for_evidence(&mut self, reduced_motion: bool) {
        self.set_reduced_motion(reduced_motion);
    }

    pub fn set_startup_logo_capabilities_for_evidence(
        &mut self,
        color_level: crate::theme::ColorLevel,
        glyph_mode: crate::theme::GlyphMode,
    ) {
        self.set_color_level(color_level);
        self.set_glyph_mode(glyph_mode);
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
        let elapsed = self.startup_motion_elapsed();
        let frames = elapsed.as_millis() / STARTUP_CADENCE.as_millis();
        usize::try_from(frames.saturating_mul(4)).unwrap_or(usize::MAX)
    }

    pub(crate) fn startup_motion_elapsed(&self) -> Duration {
        self.sampled_motion_elapsed
    }

    pub(crate) fn startup_welcome_expanded(&self) -> bool {
        self.reduced_motion || self.startup_motion_elapsed() >= STARTUP_EXPANSION_DELAY
    }

    pub(in crate::app) fn expand_startup_changelog(&mut self) {
        self.sampled_motion_elapsed = STARTUP_EXPANSION_DELAY;
        self.motion_revision = self.motion_revision.wrapping_add(1);
    }

    fn startup_welcome_transition_pending(&self) -> bool {
        !self.reduced_motion
            && self.startup_shell_visible()
            && self.welcome_visible()
            && !self.startup_welcome_expanded()
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
            && (self
                .active_permission_view()
                .is_some_and(|permission| permission.kind.eq_ignore_ascii_case("question"))
                || self.activities.iter().any(|activity| {
                    activity.status == ActivityStatus::Streaming
                        && !activity.tool_calls.iter().any(|tool| {
                            matches!(
                                tool.status,
                                ToolCallDisplayStatus::Queued
                                    | ToolCallDisplayStatus::PendingPermission
                                    | ToolCallDisplayStatus::Running
                            )
                        })
                }))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::scheduling::MotionCadence;
    use crate::theme::{ColorLevel, GlyphMode};

    use super::AppState;

    #[test]
    fn no_color_startup_still_schedules_visible_welcome_expansion() {
        // arrange
        // act
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_color_level(ColorLevel::None);

        // assert
        assert_eq!(
            app.motion_plan().cadence(),
            MotionCadence::Slow(Duration::from_millis(83))
        );
    }

    #[test]
    fn ascii_startup_still_schedules_visible_welcome_expansion() {
        // arrange
        // act
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_glyph_mode(GlyphMode::Ascii);

        // assert
        assert_eq!(
            app.motion_plan().cadence(),
            MotionCadence::Slow(Duration::from_millis(83))
        );
    }
}
