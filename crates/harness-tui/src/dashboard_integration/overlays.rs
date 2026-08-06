use crate::overlay::OverlayKind;

use super::DashboardPane;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardModal {
    Permission(String),
    Question(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardOverlayRoute {
    Modal(DashboardModalKind),
    Chrome(OverlayKind),
    Pane(DashboardPane),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardModalKind {
    Permission,
    Question,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DashboardOverlayState {
    chrome: Option<OverlayKind>,
    modal: Option<DashboardModal>,
}

impl DashboardOverlayState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open_chrome(&mut self, overlay: OverlayKind) {
        self.chrome = Some(overlay);
    }

    pub fn open_modal(&mut self, modal: DashboardModal) {
        self.modal = Some(modal);
    }

    pub fn close_modal(&mut self) -> Option<DashboardModal> {
        self.modal.take()
    }

    pub fn close_chrome(&mut self) -> Option<OverlayKind> {
        self.chrome.take()
    }

    pub const fn modal(&self) -> Option<DashboardModalKind> {
        match self.modal.as_ref() {
            Some(DashboardModal::Permission(_)) => Some(DashboardModalKind::Permission),
            Some(DashboardModal::Question(_)) => Some(DashboardModalKind::Question),
            None => None,
        }
    }

    pub const fn chrome(&self) -> Option<OverlayKind> {
        self.chrome
    }

    pub fn route(&self, pane: DashboardPane) -> DashboardOverlayRoute {
        if let Some(modal) = self.modal() {
            return DashboardOverlayRoute::Modal(modal);
        }
        if let Some(chrome) = self.chrome {
            return DashboardOverlayRoute::Chrome(chrome);
        }
        DashboardOverlayRoute::Pane(pane)
    }
}
