#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    DetailsDrawer,
    CommandPalette,
    PermissionModal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverlayState {
    pub details_drawer_open: bool,
    pub palette_visible: bool,
    pub session_history_visible: bool,
    pub model_switcher_visible: bool,
    pub permission_pending: bool,
}

impl OverlayState {
    pub fn command_palette_channel_visible(self) -> bool {
        self.palette_visible || self.session_history_visible || self.model_switcher_visible
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlayStack {
    overlays: Vec<OverlayKind>,
}

impl OverlayStack {
    pub fn from_state(state: OverlayState) -> Self {
        let mut overlays = Vec::with_capacity(3);
        if state.details_drawer_open {
            overlays.push(OverlayKind::DetailsDrawer);
        }
        if state.command_palette_channel_visible() && !state.permission_pending {
            overlays.push(OverlayKind::CommandPalette);
        }
        if state.permission_pending {
            overlays.push(OverlayKind::PermissionModal);
        }
        Self { overlays }
    }

    pub fn top(&self) -> Option<OverlayKind> {
        self.overlays.last().copied()
    }

    pub fn ordered(&self) -> &[OverlayKind] {
        &self.overlays
    }

    pub fn blocks_pointer_interaction(&self) -> bool {
        matches!(
            self.top(),
            Some(OverlayKind::CommandPalette | OverlayKind::PermissionModal)
        )
    }
}

impl<'a> IntoIterator for &'a OverlayStack {
    type Item = OverlayKind;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, OverlayKind>>;

    fn into_iter(self) -> Self::IntoIter {
        self.overlays.iter().copied()
    }
}
