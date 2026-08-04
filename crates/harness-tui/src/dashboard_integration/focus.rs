#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DashboardPane {
    Roster,
    Peek,
    Reply,
    Details,
}

impl DashboardPane {
    pub const ALL: [Self; 4] = [Self::Roster, Self::Peek, Self::Reply, Self::Details];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DashboardFocus {
    current: DashboardPane,
}

impl DashboardFocus {
    pub const fn new(current: DashboardPane) -> Self {
        Self { current }
    }

    pub const fn current(self) -> DashboardPane {
        self.current
    }

    pub fn set(&mut self, pane: DashboardPane) {
        self.current = pane;
    }

    pub fn traverse(&mut self, direction: FocusDirection, visible: &[DashboardPane]) {
        if visible.is_empty() {
            self.current = DashboardPane::Roster;
            return;
        }
        let current = visible
            .iter()
            .position(|pane| *pane == self.current)
            .unwrap_or(0);
        let next = match direction {
            FocusDirection::Forward => (current + 1) % visible.len(),
            FocusDirection::Backward => {
                if current == 0 {
                    visible.len() - 1
                } else {
                    current - 1
                }
            }
        };
        self.current = visible[next];
    }
}
