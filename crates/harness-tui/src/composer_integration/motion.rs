use crate::design_contract::{MotionKind, MotionReplacement, DESIGN_TOKENS};
use crate::scheduling::{FrameDecision, FrameInputs, FrameNow, FrameScheduler};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerMotionFrame {
    pub decision: FrameDecision,
    pub phase: u8,
    pub immediate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerMotion {
    scheduler: FrameScheduler,
    phase: u8,
}

impl ComposerMotion {
    pub const fn new(reduced_motion: bool) -> Self {
        Self {
            scheduler: FrameScheduler::with_reduced_motion(reduced_motion),
            phase: 0,
        }
    }

    pub const fn reduced_motion(self) -> bool {
        self.scheduler.reduced_motion()
    }

    pub const fn phase(self) -> u8 {
        self.phase
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.scheduler.set_reduced_motion(reduced_motion);
        if reduced_motion {
            self.phase = 0;
        }
    }

    pub fn schedule(
        &mut self,
        now: FrameNow,
        animation_active: bool,
        flush_requested: bool,
    ) -> Option<ComposerMotionFrame> {
        let decision = self.scheduler.schedule(
            now,
            FrameInputs {
                animation_active,
                flush_requested,
            },
        )?;
        let immediate = self.reduced_motion() || decision.deadline_ms.is_none();
        if decision.render && !immediate {
            self.phase = self.phase.wrapping_add(1) % spinner_frames();
        }
        Some(ComposerMotionFrame {
            decision,
            phase: self.phase,
            immediate,
        })
    }

    pub fn motion_token(kind: MotionKind) -> Option<crate::design_contract::MotionToken> {
        DESIGN_TOKENS
            .motion_tokens
            .all
            .iter()
            .find(|token| token.kind == kind)
            .copied()
    }

    pub fn reduced_substitution(
        kind: MotionKind,
    ) -> Option<crate::design_contract::ReducedMotionSubstitution> {
        DESIGN_TOKENS
            .reduced_motion_substitutions
            .all
            .iter()
            .find(|token| token.kind == kind)
            .copied()
    }

    pub fn reduced_frame(kind: MotionKind) -> Option<MotionReplacement> {
        Self::reduced_substitution(kind).map(|token| token.replacement)
    }
}

fn spinner_frames() -> u8 {
    match ComposerMotion::motion_token(MotionKind::StreamingSpinner) {
        Some(token) if token.frames > 0 => token.frames,
        Some(_) | None => 1,
    }
}

impl Default for ComposerMotion {
    fn default() -> Self {
        Self::new(false)
    }
}
