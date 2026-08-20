#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) struct TranscriptBlockId(pub(in crate::ui) String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptBlockRole {
    UserPrompt,
    AssistantBody,
    Reasoning,
    Tool,
    Footer,
    Error,
    Compaction,
    #[cfg(test)]
    Synthetic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolFamily {
    Unknown,
    Group,
    Read,
    Search,
    List,
    Execute,
    Edit,
    Web,
    Task,
    Permission,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolGroupClass {
    Commands,
    Context,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolDisclosure {
    None,
    Collapsed,
    Preview,
    Expanded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptToolStatus {
    Queued,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptSubagentMode {
    Foreground,
    Background,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptSubagentLifecycle {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptSubagentPolicy {
    pub(in crate::ui) mode: TranscriptSubagentMode,
    pub(in crate::ui) lifecycle: TranscriptSubagentLifecycle,
    pub(in crate::ui) child_session_id: Option<String>,
    pub(in crate::ui) output_truncated: bool,
    pub(in crate::ui) replay_read_only: bool,
}

impl TranscriptSubagentPolicy {
    pub(in crate::ui) fn navigation_target(&self) -> Option<&str> {
        (!self.replay_read_only)
            .then_some(self.child_session_id.as_deref())
            .flatten()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptToolPolicy {
    pub(in crate::ui) group_class: Option<TranscriptToolGroupClass>,
    pub(in crate::ui) member_count: usize,
    pub(in crate::ui) visible_start: usize,
    pub(in crate::ui) disclosure: TranscriptToolDisclosure,
    pub(in crate::ui) status: TranscriptToolStatus,
    pub(in crate::ui) motion: TranscriptBlockMotionDemand,
    pub(in crate::ui) trailing_gap_cells: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockChrome {
    pub(in crate::ui) accent: bool,
    pub(in crate::ui) rail: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockSpacing {
    pub(in crate::ui) leading_gap_rows: usize,
    pub(in crate::ui) trailing_gap_rows: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockGrouping {
    pub(in crate::ui) group_id: Option<TranscriptBlockId>,
    pub(in crate::ui) member_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockFold {
    pub(in crate::ui) foldable: bool,
    pub(in crate::ui) expanded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockInteraction {
    pub(in crate::ui) selectable: bool,
    pub(in crate::ui) selected: bool,
    pub(in crate::ui) hoverable: bool,
    pub(in crate::ui) focusable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct TranscriptBlockDisclosure {
    pub(in crate::ui) available: bool,
    pub(in crate::ui) expanded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptBlockCompactPolicy {
    Preserve,
    ElideDetails,
    Collapse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptBlockPlacement {
    Flow,
    StickyPromptCandidate,
    PinnedFooter { outdent_cells: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TranscriptBlockMotionDemand {
    None,
    Active,
    Finish,
}
