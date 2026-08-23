use super::{
    permissions::{PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage},
    Focus,
};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PermissionPointerTarget {
    Decision(PermissionModalSelection),
    Confirm(PermissionConfirmSelection),
    QuestionChoice(usize),
    QuestionSubmit,
    QuestionScrollbar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PermissionPointerDown {
    pub(crate) permission_id: String,
    pub(crate) target: PermissionPointerTarget,
    pub(crate) area: Rect,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PermissionPromptState {
    pub(crate) permission_id: Option<String>,
    pub(crate) stage: PermissionModalStage,
    pub(crate) selection: PermissionModalSelection,
    pub(crate) confirm_selection: PermissionConfirmSelection,
    pub(crate) detail_expanded: bool,
    pub(crate) pointer_down: Option<PermissionPointerDown>,
    pub(crate) focus_return: Option<Focus>,
}
