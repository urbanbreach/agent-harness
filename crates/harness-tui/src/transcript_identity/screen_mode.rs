#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InPlaceMode {
    Transcript,
    Timeline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TranscriptScreenMode {
    InPlace(InPlaceMode),
    SelectedBlockViewer,
    ExternalPagerSuspended,
}

impl TranscriptScreenMode {
    pub const fn is_in_place(self) -> bool {
        matches!(self, Self::InPlace(_))
    }
}
