use super::permissions::{
    PermissionConfirmSelection, PermissionModalSelection, PermissionModalStage,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct PermissionPromptState {
    pub(crate) permission_id: Option<String>,
    pub(crate) stage: PermissionModalStage,
    pub(crate) selection: PermissionModalSelection,
    pub(crate) confirm_selection: PermissionConfirmSelection,
}
