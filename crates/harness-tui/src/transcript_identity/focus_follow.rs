#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranscriptFocus {
    Transcript,
    Timeline,
    Viewer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FocusFollowState {
    pub focus: TranscriptFocus,
    pub follow: bool,
}

impl FocusFollowState {
    pub const fn new(focus: TranscriptFocus, follow: bool) -> Self {
        Self { focus, follow }
    }
}
