//! Multi-team durable product path (create/members/mailbox deliver/cancel).

use std::path::Path;

use crate::team_registry::{
    add_team_member_outcome, cancel_team_outcome, create_team_outcome, send_team_message_outcome,
    TeamAddMemberOutcome, TeamCancelOutcome, TeamCreateOutcome, TeamRegistrySummary,
    TeamSendOutcome, TeamStatus,
};

use super::{DurableTeamRegistry, TeamMailboxJournalError};

/// Multi-team durable product result (create/members/mailbox deliver/cancel).
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Product honesty: durable journal exists, multi-team active+cancelled, mailbox delivered.
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

/// Product path: multi-team create/members/mailbox/deliver/cancel with durable journal.
pub fn run_durable_multi_agent_team_product(
    workspace_root: &Path,
) -> Result<MultiAgentTeamProduct, TeamMailboxJournalError> {
    let mut durable = DurableTeamRegistry::open(workspace_root)?;

    let create_probe = create_team_outcome(&mut durable.registry, "(probe)");
    durable.persist()?;
    let probe_id = match &create_probe {
        TeamCreateOutcome::Created { team_id, .. } => team_id.clone(),
        TeamCreateOutcome::Failed { .. } => {
            return Ok(failed_product(durable, create_probe));
        }
    };

    let create_active = create_team_outcome(&mut durable.registry, "(probe-active)");
    durable.persist()?;
    let active_id = match &create_active {
        TeamCreateOutcome::Created { team_id, .. } => team_id.clone(),
        TeamCreateOutcome::Failed { .. } => active_id_fallback(&durable),
    };

    let _ = add_team_member_outcome(&mut durable.registry, &probe_id, "probe-agent", "operator");
    let last_add_member =
        add_team_member_outcome(&mut durable.registry, &probe_id, "probe-worker", "worker");
    durable.persist()?;

    let _ = send_team_message_outcome(
        &mut durable.registry,
        &probe_id,
        "probe-agent",
        None,
        "(probe mailbox)",
    );
    let last_send = send_team_message_outcome(
        &mut durable.registry,
        &probe_id,
        "probe-worker",
        Some("probe-agent".to_string()),
        "(probe reply)",
    );
    durable.persist()?;

    let last_message_line = durable
        .peek_inbox(&probe_id, "probe-agent")
        .ok()
        .and_then(|msgs| msgs.into_iter().last())
        .map(|msg| msg.one_line());

    let delivered = durable.deliver_messages(&probe_id, "probe-worker")?;
    let delivered_count = delivered.len();

    let last_cancel = cancel_team_outcome(&mut durable.registry, &probe_id);
    durable.persist()?;

    let _ = add_team_member_outcome(&mut durable.registry, &active_id, "probe-lead", "lead");
    let _ = send_team_message_outcome(
        &mut durable.registry,
        &active_id,
        "probe-lead",
        None,
        "(active team mailbox)",
    );
    durable.persist()?;

    let first_line = durable
        .registry
        .list_teams()
        .into_iter()
        .next()
        .map(|t| t.one_line());

    Ok(MultiAgentTeamProduct {
        summary: durable.summary(),
        last_create: create_active,
        last_add_member,
        last_send,
        last_cancel,
        delivered_count,
        journal_path: durable.journal_path().display().to_string(),
        first_line,
        last_message_line,
    })
}

fn failed_product(
    durable: DurableTeamRegistry,
    last_create: TeamCreateOutcome,
) -> MultiAgentTeamProduct {
    MultiAgentTeamProduct {
        summary: durable.summary(),
        last_create,
        last_add_member: TeamAddMemberOutcome::Failed {
            team_id: String::new(),
            agent_id: String::new(),
            reason: "skipped".to_string(),
        },
        last_send: TeamSendOutcome::Failed {
            team_id: String::new(),
            reason: "skipped".to_string(),
        },
        last_cancel: TeamCancelOutcome::Failed {
            team_id: String::new(),
            reason: "skipped".to_string(),
        },
        delivered_count: 0,
        journal_path: durable.journal_path().display().to_string(),
        first_line: None,
        last_message_line: None,
    }
}

fn active_id_fallback(durable: &DurableTeamRegistry) -> String {
    durable
        .registry
        .list_teams()
        .into_iter()
        .find(|t| t.status == TeamStatus::Active)
        .map(|t| t.team_id)
        .unwrap_or_else(|| "team_missing".to_string())
}
