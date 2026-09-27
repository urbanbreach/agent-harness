use super::BlockKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockLifecycle {
    Streaming,
    Tool,
    Waiting,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FoldState {
    Expanded,
    Collapsed,
}

impl FoldState {
    pub const fn toggle(self) -> Self {
        match self {
            Self::Expanded => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
        }
    }
}

pub const fn default_fold(kind: BlockKind, lifecycle: BlockLifecycle) -> FoldState {
    match (kind, lifecycle) {
        (BlockKind::Thinking, BlockLifecycle::Completed)
        | (BlockKind::Tool, BlockLifecycle::Completed) => FoldState::Collapsed,
        _ => FoldState::Expanded,
    }
}
