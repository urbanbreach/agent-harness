use super::*;

impl DashboardIntegration {
    pub fn begin_search(&mut self, context: SearchContext) {
        self.search.begin(context);
    }

    pub fn input_search(&mut self, text: &str) -> Result<(), DashboardIntegrationError> {
        self.search.input(text);
        self.apply_roster_search();
        Ok(())
    }

    pub fn search_details(&self, query: &str) -> Result<bool, DashboardIntegrationError> {
        let details = self
            .details
            .as_ref()
            .ok_or(DashboardIntegrationError::DetailsUnavailable)?;
        let fields = details.fields()?;
        let haystack = format!(
            "{} {} {:?} {:?} {:?} {:?}",
            fields.session_id.as_str(),
            fields.title.unwrap_or_default(),
            fields.status,
            fields.metadata.provider_model,
            fields.parent,
            fields.children
        )
        .to_lowercase();
        Ok(query
            .split_whitespace()
            .all(|term| haystack.contains(&term.to_lowercase())))
    }

    pub fn help(&self, focus: Focus) -> ShortcutHelp {
        self.input.help(match focus {
            Focus::List => DashboardPane::Roster,
            Focus::Details => DashboardPane::Peek,
            Focus::Terminal => DashboardPane::Details,
            Focus::Prompt => DashboardPane::Reply,
        })
    }

    pub fn focused_help(&self) -> ShortcutHelp {
        self.input.help(self.focus.current())
    }

    pub fn capture_return_state(&mut self, state: DashboardReturnState) {
        self.return_state = Some(state);
    }

    pub fn leave(&self) -> DashboardReturnState {
        self.return_state
            .clone()
            .unwrap_or(DashboardReturnState::new(
                TranscriptFocus::Transcript,
                true,
                None,
            ))
    }

    pub fn notify_task_completed(&mut self, session_id: &str) {
        self.hooks
            .set_title(format!("Harness dashboard · {session_id}"));
        self.hooks.notify(
            DashboardNotificationKind::TaskCompleted,
            format!("task completed: {session_id}"),
        );
    }

    pub fn focus(&self) -> DashboardPane {
        self.focus.current()
    }

    pub fn layout(&self) -> &DashboardLayout {
        &self.layout
    }

    pub fn roster_state(&self) -> &RosterState {
        &self.roster
    }

    pub fn roster_layout(&self) -> crate::dashboard_roster::RosterLayout {
        roster_layout_for_rect(self.layout.roster, &self.dashboard, &self.roster)
    }

    pub fn roster_hit_map(&self) -> RosterHitMap {
        RosterHitMap::from_layout(&self.roster_layout())
    }

    pub fn overlays(&self) -> &DashboardOverlayState {
        &self.overlays
    }

    pub fn hooks(&self) -> &DashboardHooks {
        &self.hooks
    }

    pub fn title(&self) -> &str {
        self.hooks.title()
    }

    pub fn notifications(&self) -> &[DashboardNotification] {
        self.hooks.notifications()
    }

    pub fn return_state(&self) -> Option<&DashboardReturnState> {
        self.return_state.as_ref()
    }

    pub(super) fn apply_roster_search(&mut self) {
        if self.search.context == Some(SearchContext::Roster) {
            self.roster.set_filter(
                crate::dashboard_roster::RosterFilter::new().with_query(self.search.query.clone()),
            );
        }
    }
}
