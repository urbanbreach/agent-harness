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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoldController {
    state: FoldState,
    manually_set: bool,
}

impl FoldController {
    pub const fn new(kind: BlockKind, lifecycle: BlockLifecycle) -> Self {
        Self {
            state: default_fold(kind, lifecycle),
            manually_set: false,
        }
    }

    pub const fn state(self) -> FoldState {
        self.state
    }

    pub const fn toggle(self) -> Self {
        Self {
            state: self.state.toggle(),
            manually_set: true,
        }
    }

    pub const fn lifecycle_changed(self, kind: BlockKind, lifecycle: BlockLifecycle) -> Self {
        if self.manually_set {
            self
        } else {
            Self {
                state: default_fold(kind, lifecycle),
                manually_set: false,
            }
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
