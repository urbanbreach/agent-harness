#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackProgress {
    pub current_frame: u64,
    pub total_frames: u64,
    pub elapsed_ms: u64,
    pub total_ms: u64,
}

impl PlaybackProgress {
    pub const fn new(total_frames: u64, total_ms: u64) -> Self {
        Self {
            current_frame: 0,
            total_frames,
            elapsed_ms: 0,
            total_ms,
        }
    }

    pub fn fraction_complete(&self) -> u32 {
        if self.total_frames == 0 {
            return 0;
        }
        u32::try_from((self.current_frame.min(self.total_frames) * 100) / self.total_frames)
            .unwrap_or(100)
    }

    pub fn advance(&mut self, frames: u64, ms: u64) {
        self.current_frame = self
            .current_frame
            .saturating_add(frames)
            .min(self.total_frames);
        self.elapsed_ms = self.elapsed_ms.saturating_add(ms).min(self.total_ms);
    }

    pub const fn is_complete(&self) -> bool {
        self.total_frames == 0 || self.current_frame >= self.total_frames
    }

    pub const fn eta_ms(&self) -> u64 {
        if self.elapsed_ms == 0 || self.current_frame == 0 {
            self.total_ms
        } else {
            self.total_ms.saturating_sub(self.elapsed_ms)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePacing {
    pub target_fps: u16,
    pub frame_interval_ms: u16,
}

impl FramePacing {
    pub const fn from_fps(fps: u16) -> Self {
        let safe_fps = if fps == 0 { 1 } else { fps };
        let interval = 1000 / safe_fps;
        Self {
            target_fps: safe_fps,
            frame_interval_ms: if interval == 0 { 1 } else { interval },
        }
    }

    pub const fn for_width(width: u16) -> Self {
        if width <= 80 {
            Self::from_fps(30)
        } else if width <= 132 {
            Self::from_fps(24)
        } else {
            Self::from_fps(15)
        }
    }

    pub const fn default_pacing() -> Self {
        Self {
            target_fps: 24,
            frame_interval_ms: 41,
        }
    }
}
