//! Minimal multi-agent team registry (coordinator-owned types).
//!
//! Provides create/add-member/list/cancel plus an in-memory per-team mailbox.
//! This is not Team Mode product parity (no process/workspace coordination).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Lifecycle state for a registered team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamStatus {
    Active,
    Cancelled,
}

impl TeamStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One member of a team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub agent_id: String,
    pub role: String,
}

/// Registered team record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRecord {
    pub team_id: String,
    pub name: String,
    pub status: TeamStatus,
    pub members: Vec<TeamMember>,
}

impl TeamRecord {
    /// Operator-facing one-line diagnostics (does not claim Team Mode product).
    pub fn one_line(&self) -> String {
        format!(
            "team `{}` name=`{}` status={} members={}",
            self.team_id,
            self.name,
            self.status.as_str(),
            self.members.len()
        )
    }
}

/// One in-memory mailbox message (not process IPC).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMessage {
    pub message_id: String,
    pub team_id: String,
    pub from_agent_id: String,
    /// When `None`, the message is a team broadcast.
    pub to_agent_id: Option<String>,
    pub body: String,
    pub seq: u64,
}

impl TeamMessage {
    /// Operator-facing one-line diagnostics (does not claim process IPC).
    pub fn one_line(&self) -> String {
        let to = self
            .to_agent_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("*");
        let body_hint: String = self.body.chars().take(24).collect();
        format!(
            "team msg `{}` team=`{}` from=`{}` to=`{}` seq={} body=`{}`",
            self.message_id, self.team_id, self.from_agent_id, to, self.seq, body_hint
        )
    }
}

/// Snapshot of [`TeamRegistry`] fields for durable journal restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRegistryParts {
    pub teams: BTreeMap<String, TeamRecord>,
    pub mailboxes: BTreeMap<String, Vec<TeamMessage>>,
    pub next_seq: u64,
    pub next_message_seq: u64,
}

/// In-memory team registry owned by coordinator-side orchestration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamRegistry {
    teams: BTreeMap<String, TeamRecord>,
    /// Per-team ordered mailbox (append-only until drained by receive).
    mailboxes: BTreeMap<String, Vec<TeamMessage>>,
    next_seq: u64,
    next_message_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TeamRegistryError {
    #[error("team name must be non-empty")]
    EmptyName,
    #[error("team `{team_id}` not found")]
    NotFound { team_id: String },
    #[error("team `{team_id}` is cancelled")]
    Cancelled { team_id: String },
    #[error("agent_id must be non-empty")]
    EmptyAgentId,
    #[error("agent `{agent_id}` already on team `{team_id}`")]
    DuplicateMember { team_id: String, agent_id: String },
    #[error("agent `{agent_id}` is not a member of team `{team_id}`")]
    NotAMember { team_id: String, agent_id: String },
    #[error("message body must be non-empty")]
    EmptyMessageBody,
}

/// Operator-facing counts for a team registry (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TeamRegistrySummary {
    pub teams: usize,
    pub active: usize,
    pub cancelled: usize,
    pub members: usize,
    pub mailbox_messages: usize,
}

impl TeamRegistrySummary {
    pub fn one_line(&self) -> String {
        format!(
            "teams: {} total ({} active, {} cancelled; {} members; {} mailbox msgs)",
            self.teams, self.active, self.cancelled, self.members, self.mailbox_messages
        )
    }

    pub const fn has_active(&self) -> bool {
        self.active > 0
    }
}

impl TeamRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reconstruct from a durable snapshot (mailbox journal restore).
    pub fn from_parts(parts: TeamRegistryParts) -> Self {
        Self {
            teams: parts.teams,
            mailboxes: parts.mailboxes,
            next_seq: parts.next_seq,
            next_message_seq: parts.next_message_seq,
        }
    }

    /// Export fields for durable journal persistence.
    pub fn to_parts(&self) -> TeamRegistryParts {
        TeamRegistryParts {
            teams: self.teams.clone(),
            mailboxes: self.mailboxes.clone(),
            next_seq: self.next_seq,
            next_message_seq: self.next_message_seq,
        }
    }

    pub fn create_team(
        &mut self,
        name: impl Into<String>,
    ) -> Result<TeamRecord, TeamRegistryError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(TeamRegistryError::EmptyName);
        }
        self.next_seq = self.next_seq.saturating_add(1);
        let team_id = format!("team_{}", self.next_seq);
        let record = TeamRecord {
            team_id: team_id.clone(),
            name: name.trim().to_string(),
            status: TeamStatus::Active,
            members: Vec::new(),
        };
        self.teams.insert(team_id.clone(), record.clone());
        self.mailboxes.entry(team_id).or_default();
        Ok(record)
    }

    pub fn add_member(
        &mut self,
        team_id: &str,
        agent_id: impl Into<String>,
        role: impl Into<String>,
    ) -> Result<TeamRecord, TeamRegistryError> {
        let agent_id = agent_id.into();
        if agent_id.trim().is_empty() {
            return Err(TeamRegistryError::EmptyAgentId);
        }
        let team = self
            .teams
            .get_mut(team_id)
            .ok_or_else(|| TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            })?;
        if team.status == TeamStatus::Cancelled {
            return Err(TeamRegistryError::Cancelled {
                team_id: team_id.to_string(),
            });
        }
        if team.members.iter().any(|m| m.agent_id == agent_id) {
            return Err(TeamRegistryError::DuplicateMember {
                team_id: team_id.to_string(),
                agent_id,
            });
        }
        team.members.push(TeamMember {
            agent_id: agent_id.trim().to_string(),
            role: role.into(),
        });
        Ok(team.clone())
    }

    pub fn list_teams(&self) -> Vec<TeamRecord> {
        self.teams.values().cloned().collect()
    }

    pub fn get_team(&self, team_id: &str) -> Option<&TeamRecord> {
        self.teams.get(team_id)
    }

    pub fn list_members(&self, team_id: &str) -> Result<Vec<TeamMember>, TeamRegistryError> {
        let team = self
            .teams
            .get(team_id)
            .ok_or_else(|| TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            })?;
        Ok(team.members.clone())
    }

    pub fn remove_member(
        &mut self,
        team_id: &str,
        agent_id: impl Into<String>,
    ) -> Result<TeamRecord, TeamRegistryError> {
        let agent_id = agent_id.into();
        if agent_id.trim().is_empty() {
            return Err(TeamRegistryError::EmptyAgentId);
        }
        let team = self
            .teams
            .get_mut(team_id)
            .ok_or_else(|| TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            })?;
        if team.status == TeamStatus::Cancelled {
            return Err(TeamRegistryError::Cancelled {
                team_id: team_id.to_string(),
            });
        }
        let before = team.members.len();
        team.members.retain(|m| m.agent_id != agent_id.trim());
        if team.members.len() == before {
            return Err(TeamRegistryError::NotAMember {
                team_id: team_id.to_string(),
                agent_id: agent_id.trim().to_string(),
            });
        }
        Ok(team.clone())
    }

    pub fn cancel_team(&mut self, team_id: &str) -> Result<TeamRecord, TeamRegistryError> {
        let team = self
            .teams
            .get_mut(team_id)
            .ok_or_else(|| TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            })?;
        team.status = TeamStatus::Cancelled;
        Ok(team.clone())
    }

    /// Post a message to a team's in-memory mailbox.
    ///
    /// `to_agent_id = None` is a broadcast. Sender and optional recipient must
    /// already be team members. Cancelled teams reject posts.
    pub fn send_message(
        &mut self,
        team_id: &str,
        from_agent_id: impl Into<String>,
        to_agent_id: Option<String>,
        body: impl Into<String>,
    ) -> Result<TeamMessage, TeamRegistryError> {
        let from_agent_id = from_agent_id.into();
        let body = body.into();
        if from_agent_id.trim().is_empty() {
            return Err(TeamRegistryError::EmptyAgentId);
        }
        if body.trim().is_empty() {
            return Err(TeamRegistryError::EmptyMessageBody);
        }
        let team = self
            .teams
            .get(team_id)
            .ok_or_else(|| TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            })?;
        if team.status == TeamStatus::Cancelled {
            return Err(TeamRegistryError::Cancelled {
                team_id: team_id.to_string(),
            });
        }
        if !team.members.iter().any(|m| m.agent_id == from_agent_id) {
            return Err(TeamRegistryError::NotAMember {
                team_id: team_id.to_string(),
                agent_id: from_agent_id,
            });
        }
        if let Some(ref to) = to_agent_id {
            if to.trim().is_empty() {
                return Err(TeamRegistryError::EmptyAgentId);
            }
            if !team.members.iter().any(|m| m.agent_id == *to) {
                return Err(TeamRegistryError::NotAMember {
                    team_id: team_id.to_string(),
                    agent_id: to.clone(),
                });
            }
        }

        self.next_message_seq = self.next_message_seq.saturating_add(1);
        let message = TeamMessage {
            message_id: format!("msg_{}", self.next_message_seq),
            team_id: team_id.to_string(),
            from_agent_id: from_agent_id.trim().to_string(),
            to_agent_id: to_agent_id.map(|id| id.trim().to_string()),
            body: body.trim().to_string(),
            seq: self.next_message_seq,
        };
        self.mailboxes
            .entry(team_id.to_string())
            .or_default()
            .push(message.clone());
        Ok(message)
    }

    /// Peek undelivered mailbox messages for an agent without draining.
    ///
    /// Includes direct messages to `agent_id` and broadcasts (`to_agent_id = None`).
    pub fn peek_inbox(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TeamMessage>, TeamRegistryError> {
        self.require_active_member(team_id, agent_id)?;
        let messages = self
            .mailboxes
            .get(team_id)
            .map(|msgs| {
                msgs.iter()
                    .filter(|msg| match &msg.to_agent_id {
                        None => true,
                        Some(to) => to == agent_id,
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(messages)
    }

    /// Drain undelivered mailbox messages for an agent (receive + remove).
    ///
    /// Direct messages to other agents remain. Broadcasts remain until every
    /// active member has received them via this drain path; for honesty this MVP
    /// removes a broadcast only when the last remaining recipient for that
    /// message is the draining agent (i.e. no other active member still needs it).
    /// Simpler MVP: remove messages addressed to this agent, and remove broadcasts
    /// only after all active members have drained them — tracked by per-message
    /// receipt set would be fuller product. Here: drain removes directed messages
    /// for the agent and leaves broadcasts in place for other members (broadcast
    /// is re-delivered on peek/receive until team cancel). That would re-deliver.
    ///
    /// Practical MVP chosen: receive removes messages where `to == agent` OR
    /// `to == None` (broadcast delivered once per receive call per agent is not
    /// tracked). To avoid multi-receive of the same broadcast, broadcasts are
    /// removed on first receive by any member. This is intentional honest MVP
    /// (not durable multi-consumer fanout).
    pub fn receive_messages(
        &mut self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TeamMessage>, TeamRegistryError> {
        self.require_active_member(team_id, agent_id)?;
        let mailbox = self.mailboxes.entry(team_id.to_string()).or_default();
        let mut delivered = Vec::new();
        let mut retained = Vec::new();
        for msg in mailbox.drain(..) {
            let for_agent = match &msg.to_agent_id {
                None => true,
                Some(to) => to == agent_id,
            };
            if for_agent {
                delivered.push(msg);
            } else {
                retained.push(msg);
            }
        }
        *mailbox = retained;
        Ok(delivered)
    }

    /// Count of undelivered messages currently in a team's mailbox.
    pub fn mailbox_len(&self, team_id: &str) -> Result<usize, TeamRegistryError> {
        if !self.teams.contains_key(team_id) {
            return Err(TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            });
        }
        Ok(self.mailboxes.get(team_id).map(Vec::len).unwrap_or(0))
    }

    /// Operator-facing counts for registered teams (diagnostics only).
    pub fn summary(&self) -> TeamRegistrySummary {
        let mut summary = TeamRegistrySummary {
            teams: self.teams.len(),
            ..TeamRegistrySummary::default()
        };
        for team in self.teams.values() {
            match team.status {
                TeamStatus::Active => {
                    summary.active = summary.active.saturating_add(1);
                }
                TeamStatus::Cancelled => {
                    summary.cancelled = summary.cancelled.saturating_add(1);
                }
            }
            summary.members = summary.members.saturating_add(team.members.len());
            summary.mailbox_messages = summary
                .mailbox_messages
                .saturating_add(self.mailboxes.get(&team.team_id).map(Vec::len).unwrap_or(0));
        }
        summary
    }

    fn require_active_member(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<(), TeamRegistryError> {
        if agent_id.trim().is_empty() {
            return Err(TeamRegistryError::EmptyAgentId);
        }
        let team = self
            .teams
            .get(team_id)
            .ok_or_else(|| TeamRegistryError::NotFound {
                team_id: team_id.to_string(),
            })?;
        if team.status == TeamStatus::Cancelled {
            return Err(TeamRegistryError::Cancelled {
                team_id: team_id.to_string(),
            });
        }
        if !team.members.iter().any(|m| m.agent_id == agent_id) {
            return Err(TeamRegistryError::NotAMember {
                team_id: team_id.to_string(),
                agent_id: agent_id.to_string(),
            });
        }
        Ok(())
    }
}

/// Result of a single team create attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamCreateOutcome {
    Created { team_id: String, name: String },
    Failed { name: String, reason: String },
}

impl TeamCreateOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Created { team_id, name } => {
                format!("team create: ok id=`{team_id}` name=`{name}`")
            }
            Self::Failed { name, reason } => {
                format!("team create: failed name=`{name}` ({reason})")
            }
        }
    }
}

/// Create a team and return a structured operator-facing outcome.
pub fn create_team_outcome(
    registry: &mut TeamRegistry,
    name: impl Into<String>,
) -> TeamCreateOutcome {
    let name = name.into();
    match registry.create_team(name.clone()) {
        Ok(record) => TeamCreateOutcome::Created {
            team_id: record.team_id,
            name: record.name,
        },
        Err(err) => TeamCreateOutcome::Failed {
            name,
            reason: err.to_string(),
        },
    }
}

/// Result of a single team mailbox send attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamSendOutcome {
    Sent {
        message_id: String,
        team_id: String,
        from_agent_id: String,
    },
    Failed {
        team_id: String,
        reason: String,
    },
}

impl TeamSendOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Sent {
                message_id,
                team_id,
                from_agent_id,
            } => {
                format!("team send: ok msg=`{message_id}` team=`{team_id}` from=`{from_agent_id}`")
            }
            Self::Failed { team_id, reason } => {
                format!("team send: failed team=`{team_id}` ({reason})")
            }
        }
    }
}

/// Send a team mailbox message and return a structured operator-facing outcome.
pub fn send_team_message_outcome(
    registry: &mut TeamRegistry,
    team_id: &str,
    from_agent_id: impl Into<String>,
    to_agent_id: Option<String>,
    body: impl Into<String>,
) -> TeamSendOutcome {
    let team_id_owned = team_id.to_string();
    match registry.send_message(team_id, from_agent_id, to_agent_id, body) {
        Ok(message) => TeamSendOutcome::Sent {
            message_id: message.message_id,
            team_id: message.team_id,
            from_agent_id: message.from_agent_id,
        },
        Err(err) => TeamSendOutcome::Failed {
            team_id: team_id_owned,
            reason: err.to_string(),
        },
    }
}

/// Result of a single team add-member attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamAddMemberOutcome {
    Added {
        team_id: String,
        agent_id: String,
        role: String,
        member_count: usize,
    },
    Failed {
        team_id: String,
        agent_id: String,
        reason: String,
    },
}

impl TeamAddMemberOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Added {
                team_id,
                agent_id,
                role,
                member_count,
            } => format!(
                "team add-member: ok team=`{team_id}` agent=`{agent_id}` role=`{role}` members={member_count}"
            ),
            Self::Failed {
                team_id,
                agent_id,
                reason,
            } => format!(
                "team add-member: failed team=`{team_id}` agent=`{agent_id}` ({reason})"
            ),
        }
    }
}

/// Add a team member and return a structured operator-facing outcome.
pub fn add_team_member_outcome(
    registry: &mut TeamRegistry,
    team_id: &str,
    agent_id: impl Into<String>,
    role: impl Into<String>,
) -> TeamAddMemberOutcome {
    let agent_id = agent_id.into();
    let role = role.into();
    match registry.add_member(team_id, agent_id.clone(), role.clone()) {
        Ok(record) => TeamAddMemberOutcome::Added {
            team_id: record.team_id,
            agent_id,
            role,
            member_count: record.members.len(),
        },
        Err(err) => TeamAddMemberOutcome::Failed {
            team_id: team_id.to_string(),
            agent_id,
            reason: err.to_string(),
        },
    }
}

/// Result of a single team cancel attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TeamCancelOutcome {
    Cancelled { team_id: String, name: String },
    Failed { team_id: String, reason: String },
}

impl TeamCancelOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Cancelled { team_id, name } => {
                format!("team cancel: ok id=`{team_id}` name=`{name}`")
            }
            Self::Failed { team_id, reason } => {
                format!("team cancel: failed id=`{team_id}` ({reason})")
            }
        }
    }
}

/// Cancel a team and return a structured operator-facing outcome.
pub fn cancel_team_outcome(registry: &mut TeamRegistry, team_id: &str) -> TeamCancelOutcome {
    match registry.cancel_team(team_id) {
        Ok(record) => TeamCancelOutcome::Cancelled {
            team_id: record.team_id,
            name: record.name,
        },
        Err(err) => TeamCancelOutcome::Failed {
            team_id: team_id.to_string(),
            reason: err.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_add_list_cancel_team_lifecycle() {
        // Given
        let mut registry = TeamRegistry::new();

        // When: create
        let team = registry.create_team("alpha").expect("create");
        assert_eq!(team.status, TeamStatus::Active);
        assert!(team.members.is_empty());

        // When: add members
        let team = registry
            .add_member(&team.team_id, "agent_a", "lead")
            .expect("add lead");
        let team = registry
            .add_member(&team.team_id, "agent_b", "worker")
            .expect("add worker");
        assert_eq!(team.members.len(), 2);

        // Then: list
        let listed = registry.list_teams();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].members.len(), 2);

        // When: cancel
        let cancelled = registry.cancel_team(&team.team_id).expect("cancel");
        assert_eq!(cancelled.status, TeamStatus::Cancelled);

        // Then: cannot add after cancel
        let err = registry
            .add_member(&team.team_id, "agent_c", "late")
            .expect_err("cancelled");
        assert!(matches!(err, TeamRegistryError::Cancelled { .. }));
    }

    #[test]
    fn create_team_rejects_empty_name() {
        let mut registry = TeamRegistry::new();
        let err = registry.create_team("  ").expect_err("empty");
        assert_eq!(err, TeamRegistryError::EmptyName);
    }

    #[test]
    fn add_member_rejects_duplicates() {
        let mut registry = TeamRegistry::new();
        let team = registry.create_team("dup").expect("create");
        registry
            .add_member(&team.team_id, "agent_a", "lead")
            .expect("first");
        let err = registry
            .add_member(&team.team_id, "agent_a", "other")
            .expect_err("dup");
        assert!(matches!(err, TeamRegistryError::DuplicateMember { .. }));
    }

    #[test]
    fn mailbox_send_peek_receive_direct_and_broadcast() {
        // Given: active team with two members
        let mut registry = TeamRegistry::new();
        let team = registry.create_team("mail").expect("create");
        registry
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");
        registry
            .add_member(&team.team_id, "worker", "worker")
            .expect("worker");

        // When: direct + broadcast
        let direct = registry
            .send_message(
                &team.team_id,
                "lead",
                Some("worker".to_string()),
                "do the thing",
            )
            .expect("direct");
        let broadcast = registry
            .send_message(&team.team_id, "lead", None, "standup now")
            .expect("broadcast");
        assert_eq!(registry.mailbox_len(&team.team_id).expect("len"), 2);

        // Then: worker peeks both; lead peeks only broadcast
        let worker_peek = registry
            .peek_inbox(&team.team_id, "worker")
            .expect("worker peek");
        assert_eq!(worker_peek.len(), 2);
        assert_eq!(worker_peek[0].message_id, direct.message_id);
        assert_eq!(worker_peek[1].message_id, broadcast.message_id);

        let lead_peek = registry
            .peek_inbox(&team.team_id, "lead")
            .expect("lead peek");
        assert_eq!(lead_peek.len(), 1);
        assert_eq!(lead_peek[0].message_id, broadcast.message_id);

        // When: worker receives (drains directed + broadcast per MVP)
        let worker_recv = registry
            .receive_messages(&team.team_id, "worker")
            .expect("worker recv");
        assert_eq!(worker_recv.len(), 2);
        assert_eq!(registry.mailbox_len(&team.team_id).expect("len"), 0);

        // Then: lead receives nothing after broadcast drained by worker
        let lead_recv = registry
            .receive_messages(&team.team_id, "lead")
            .expect("lead recv");
        assert!(lead_recv.is_empty());
    }

    #[test]
    fn mailbox_rejects_non_member_and_cancelled_team() {
        let mut registry = TeamRegistry::new();
        let team = registry.create_team("strict").expect("create");
        registry
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");

        let err = registry
            .send_message(&team.team_id, "ghost", None, "hi")
            .expect_err("non-member");
        assert!(matches!(err, TeamRegistryError::NotAMember { .. }));

        let err = registry
            .send_message(&team.team_id, "lead", None, "   ")
            .expect_err("empty body");
        assert_eq!(err, TeamRegistryError::EmptyMessageBody);

        registry.cancel_team(&team.team_id).expect("cancel");
        let err = registry
            .send_message(&team.team_id, "lead", None, "late")
            .expect_err("cancelled");
        assert!(matches!(err, TeamRegistryError::Cancelled { .. }));
    }

    #[test]
    fn list_and_remove_member_round_trip() {
        // Given
        let mut registry = TeamRegistry::new();
        let team = registry.create_team("crew").expect("create");
        registry
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");
        registry
            .add_member(&team.team_id, "worker", "worker")
            .expect("worker");

        // When
        let members = registry.list_members(&team.team_id).expect("list");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].agent_id, "lead");
        assert_eq!(members[1].agent_id, "worker");

        let after = registry
            .remove_member(&team.team_id, "worker")
            .expect("remove");

        // Then
        assert_eq!(after.members.len(), 1);
        assert_eq!(after.members[0].agent_id, "lead");
        let listed = registry.list_members(&team.team_id).expect("list again");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].agent_id, "lead");
    }

    #[test]
    fn remove_member_fail_closed_for_missing_cancelled_and_empty() {
        // Given
        let mut registry = TeamRegistry::new();
        let team = registry.create_team("strict-rm").expect("create");
        registry
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");

        // When / Then missing member
        let err = registry
            .remove_member(&team.team_id, "ghost")
            .expect_err("missing");
        assert!(matches!(err, TeamRegistryError::NotAMember { .. }));

        // When / Then empty agent
        let err = registry
            .remove_member(&team.team_id, "  ")
            .expect_err("empty");
        assert_eq!(err, TeamRegistryError::EmptyAgentId);

        // When / Then cancelled team
        registry.cancel_team(&team.team_id).expect("cancel");
        let err = registry
            .remove_member(&team.team_id, "lead")
            .expect_err("cancelled");
        assert!(matches!(err, TeamRegistryError::Cancelled { .. }));

        // When / Then unknown team
        let err = registry
            .list_members("team_missing")
            .expect_err("not found");
        assert!(matches!(err, TeamRegistryError::NotFound { .. }));
    }

    #[test]
    fn team_registry_summary_counts_active_cancelled_members_and_mailbox() {
        // Given: one active team with members + mailbox, one cancelled team
        let mut registry = TeamRegistry::new();
        let active = registry.create_team("alpha").expect("create active");
        registry
            .add_member(&active.team_id, "lead", "lead")
            .expect("lead");
        registry
            .add_member(&active.team_id, "worker", "worker")
            .expect("worker");
        registry
            .send_message(&active.team_id, "lead", None, "hello team")
            .expect("broadcast");
        let cancelled = registry.create_team("beta").expect("create beta");
        registry
            .add_member(&cancelled.team_id, "solo", "solo")
            .expect("solo");
        registry.cancel_team(&cancelled.team_id).expect("cancel");

        // When
        let summary = registry.summary();

        // Then
        assert_eq!(
            summary,
            TeamRegistrySummary {
                teams: 2,
                active: 1,
                cancelled: 1,
                members: 3,
                mailbox_messages: 1,
            }
        );
        assert!(summary.has_active());
        assert!(summary.one_line().contains("2 total"));
        assert!(summary.one_line().contains("1 active"));
        assert!(summary.one_line().contains("1 cancelled"));
        assert_eq!(TeamRegistry::new().summary().teams, 0);
    }

    #[test]
    fn multi_team_create_members_mailbox_and_cancel_outcomes() {
        // Given: empty multi-team registry
        let mut registry = TeamRegistry::new();

        // When: create (probe) + (probe-active); probe gets 2 members + 2 mailbox msgs then cancel
        let create = create_team_outcome(&mut registry, "(probe)");
        let TeamCreateOutcome::Created {
            team_id: probe_id, ..
        } = create
        else {
            panic!("expected probe create ok: {create:?}");
        };
        let create_active = create_team_outcome(&mut registry, "(probe-active)");
        let TeamCreateOutcome::Created {
            team_id: active_id, ..
        } = create_active
        else {
            panic!("expected active create ok: {create_active:?}");
        };

        let add_lead = add_team_member_outcome(&mut registry, &probe_id, "probe-agent", "operator");
        let add_worker =
            add_team_member_outcome(&mut registry, &probe_id, "probe-worker", "worker");
        assert!(matches!(
            add_lead,
            TeamAddMemberOutcome::Added {
                member_count: 1,
                ..
            }
        ));
        assert!(matches!(
            add_worker,
            TeamAddMemberOutcome::Added {
                member_count: 2,
                ..
            }
        ));

        let send_broadcast = send_team_message_outcome(
            &mut registry,
            &probe_id,
            "probe-agent",
            None,
            "(probe mailbox)",
        );
        let send_direct = send_team_message_outcome(
            &mut registry,
            &probe_id,
            "probe-worker",
            Some("probe-agent".to_string()),
            "(probe reply)",
        );
        assert!(matches!(send_broadcast, TeamSendOutcome::Sent { .. }));
        assert!(matches!(send_direct, TeamSendOutcome::Sent { .. }));
        assert_eq!(registry.mailbox_len(&probe_id).expect("mailbox"), 2);

        let cancel = cancel_team_outcome(&mut registry, &probe_id);
        assert!(matches!(
            cancel,
            TeamCancelOutcome::Cancelled { team_id, .. } if team_id == probe_id
        ));

        // When: active team gets lead member + mailbox (stays active)
        let add_active = add_team_member_outcome(&mut registry, &active_id, "probe-lead", "lead");
        assert!(matches!(add_active, TeamAddMemberOutcome::Added { .. }));
        let send_active = send_team_message_outcome(
            &mut registry,
            &active_id,
            "probe-lead",
            None,
            "(active team mailbox)",
        );
        assert!(matches!(send_active, TeamSendOutcome::Sent { .. }));

        // Then: summary teams>=2 active>=1 cancelled>=1 members>=3 mailbox>=2
        let summary = registry.summary();
        assert!(
            summary.teams >= 2 && summary.active >= 1 && summary.cancelled >= 1,
            "expected multi-team active+cancelled: {summary:?}"
        );
        assert!(
            summary.members >= 3,
            "expected multi-member teams: {summary:?}"
        );
        assert!(
            summary.mailbox_messages >= 2,
            "expected multi-message mailbox: {summary:?}"
        );
        let members = registry.list_members(&active_id).expect("active members");
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].agent_id, "probe-lead");
        let cancelled_err = registry
            .add_member(&probe_id, "late", "late")
            .expect_err("cancelled probe");
        assert!(matches!(cancelled_err, TeamRegistryError::Cancelled { .. }));
    }
}
