use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiAgentTeamProduct {
    pub summary: TeamRegistrySummary,
    pub last_create: TeamCreateOutcome,
    pub last_add_member: TeamAddMemberOutcome,
    pub last_send: TeamSendOutcome,
    pub last_cancel: TeamCancelOutcome,
    pub delivered_count: usize,
    pub journal_path: String,
    pub first_line: Option<String>,
    pub last_message_line: Option<String>,
}
impl MultiAgentTeamProduct {
    pub fn meets_durable_team_contract(&self) -> bool {
        self.summary.teams >= 2
            && self.summary.active >= 1
            && self.summary.cancelled >= 1
            && self.summary.members >= 3
            && self.delivered_count >= 1
            && matches!(self.last_create, TeamCreateOutcome::Created { .. })
            && matches!(self.last_add_member, TeamAddMemberOutcome::Added { .. })
            && matches!(self.last_send, TeamSendOutcome::Sent { .. })
            && matches!(self.last_cancel, TeamCancelOutcome::Cancelled { .. })
            && Path::new(&self.journal_path).is_file()
    }
}
/// Explicit diagnostic fixture used by the TUI tests, never a startup probe.
pub fn run_durable_multi_agent_team_product(
    root: &Path,
) -> Result<MultiAgentTeamProduct, TeamMailboxJournalError> {
    let mut durable = DurableTeamRegistry::open(root)?;
    let journal_path = durable.journal_path.display().to_string();
    durable.update(|registry| {
        let cancelled = registry.create_team("(probe)")?;
        let active = registry.create_team("(probe-active)")?;
        registry.add_member(&cancelled.team_id, "probe-agent", "operator")?;
        let added = registry.add_member(&cancelled.team_id, "probe-worker", "worker")?;
        registry.add_member(&active.team_id, "probe-lead", "lead")?;
        registry.send_message(&cancelled.team_id, "probe-agent", None, "(probe mailbox)")?;
        let sent = registry.send_message(
            &cancelled.team_id,
            "probe-worker",
            Some("probe-agent".into()),
            "(probe reply)",
        )?;
        let delivered_count = registry
            .receive_messages(&cancelled.team_id, "probe-worker")?
            .len();
        let last_message_line = Some(sent.one_line());
        let cancelled = registry.cancel_team(&cancelled.team_id)?;
        registry.send_message(&active.team_id, "probe-lead", None, "(active team mailbox)")?;
        Ok(MultiAgentTeamProduct {
            summary: registry.summary(),
            last_create: TeamCreateOutcome::Created {
                team_id: active.team_id,
                name: active.name,
            },
            last_add_member: TeamAddMemberOutcome::Added {
                team_id: added.team_id,
                agent_id: "probe-worker".into(),
                role: "worker".into(),
                member_count: added.members.len(),
            },
            last_send: TeamSendOutcome::Sent {
                message_id: sent.message_id,
                team_id: sent.team_id,
                from_agent_id: sent.from_agent_id,
            },
            last_cancel: TeamCancelOutcome::Cancelled {
                team_id: cancelled.team_id,
                name: cancelled.name,
            },
            delivered_count,
            journal_path,
            first_line: registry.list_teams().first().map(TeamRecord::one_line),
            last_message_line,
        })
    })
}
