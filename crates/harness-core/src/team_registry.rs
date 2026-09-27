use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod outcomes;
mod validate;
pub use outcomes::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamStatus {
    Active,
    Cancelled,
}
impl TeamStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Cancelled => "cancelled",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub agent_id: String,
    pub role: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRecord {
    pub team_id: String,
    pub name: String,
    pub status: TeamStatus,
    pub members: Vec<TeamMember>,
}
impl TeamRecord {
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMessage {
    pub message_id: String,
    pub team_id: String,
    pub from_agent_id: String,
    pub to_agent_id: Option<String>,
    pub body: String,
    pub seq: u64,
}
impl TeamMessage {
    pub fn one_line(&self) -> String {
        let hint: String = crate::redact::redact_artifact_text(&self.body)
            .chars()
            .take(24)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        format!(
            "team msg `{}` team=`{}` from=`{}` to=`{}` seq={} body=`{hint}`",
            self.message_id,
            self.team_id,
            self.from_agent_id,
            self.to_agent_id.as_deref().unwrap_or("*"),
            self.seq
        )
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TeamRegistryParts {
    pub teams: BTreeMap<String, TeamRecord>,
    pub mailboxes: BTreeMap<String, Vec<TeamMessage>>,
    pub next_seq: u64,
    pub next_message_seq: u64,
}
#[derive(Clone, Debug, Default)]
pub struct TeamRegistry {
    parts: TeamRegistryParts,
}
#[derive(Debug, thiserror::Error)]
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
    #[error("team text is too long or contains control characters")]
    InvalidText,
    #[error("team registry capacity exceeded")]
    Capacity,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn has_active(&self) -> bool {
        self.active > 0
    }
}
impl TeamRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_parts(mut parts: TeamRegistryParts) -> Self {
        parts.next_seq = parts.next_seq.max(
            parts
                .teams
                .keys()
                .filter_map(|id| id.strip_prefix("team_")?.parse().ok())
                .max()
                .unwrap_or(0),
        );
        parts.next_message_seq = parts.next_message_seq.max(
            parts
                .mailboxes
                .values()
                .flatten()
                .map(|m| m.seq)
                .max()
                .unwrap_or(0),
        );
        Self { parts }
    }
    pub fn to_parts(&self) -> TeamRegistryParts {
        self.parts.clone()
    }
    pub(crate) fn into_parts(self) -> TeamRegistryParts {
        self.parts
    }
    pub fn create_team(
        &mut self,
        name: impl Into<String>,
    ) -> Result<TeamRecord, TeamRegistryError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(TeamRegistryError::EmptyName);
        }
        check_label(&name)?;
        if self.parts.teams.len() >= 256 {
            return Err(TeamRegistryError::Capacity);
        }
        let seq = self
            .parts
            .next_seq
            .checked_add(1)
            .ok_or(TeamRegistryError::Capacity)?;
        let team = TeamRecord {
            team_id: format!("team_{seq}"),
            name: name.trim().into(),
            status: TeamStatus::Active,
            members: Vec::new(),
        };
        if self.parts.teams.contains_key(&team.team_id) {
            return Err(TeamRegistryError::Capacity);
        }
        self.parts.teams.insert(team.team_id.clone(), team.clone());
        self.parts.next_seq = seq;
        Ok(team)
    }
    pub fn add_member(
        &mut self,
        team_id: &str,
        agent_id: impl Into<String>,
        role: impl Into<String>,
    ) -> Result<TeamRecord, TeamRegistryError> {
        let agent_id = agent_id.into();
        let agent_id = agent_id.trim();
        if agent_id.is_empty() {
            return Err(TeamRegistryError::EmptyAgentId);
        }
        let role = role.into();
        check_label(agent_id)?;
        check_label(&role)?;
        let team = self.active(team_id)?;
        if team.members.iter().any(|m| m.agent_id == agent_id) {
            return Err(TeamRegistryError::DuplicateMember {
                team_id: team_id.into(),
                agent_id: agent_id.into(),
            });
        }
        if team.members.len() >= 256 {
            return Err(TeamRegistryError::Capacity);
        }
        team.members.push(TeamMember {
            agent_id: agent_id.into(),
            role: role.trim().into(),
        });
        Ok(team.clone())
    }
    pub fn list_teams(&self) -> Vec<TeamRecord> {
        self.parts.teams.values().cloned().collect()
    }
    pub fn get_team(&self, team_id: &str) -> Option<&TeamRecord> {
        self.parts.teams.get(team_id)
    }
    pub fn list_members(&self, team_id: &str) -> Result<Vec<TeamMember>, TeamRegistryError> {
        Ok(self.team(team_id)?.members.clone())
    }
    pub fn remove_member(
        &mut self,
        team_id: &str,
        agent_id: impl Into<String>,
    ) -> Result<TeamRecord, TeamRegistryError> {
        let agent_id = agent_id.into();
        self.require_member(team_id, agent_id.trim())?;
        let team = self.active(team_id)?;
        team.members.retain(|m| m.agent_id != agent_id.trim());
        let team = team.clone();
        if let Some(messages) = self.parts.mailboxes.get_mut(team_id) {
            messages.retain(|m| m.to_agent_id.as_deref() != Some(agent_id.trim()));
        }
        Ok(team)
    }
    pub fn cancel_team(&mut self, team_id: &str) -> Result<TeamRecord, TeamRegistryError> {
        let team =
            self.parts
                .teams
                .get_mut(team_id)
                .ok_or_else(|| TeamRegistryError::NotFound {
                    team_id: team_id.into(),
                })?;
        team.status = TeamStatus::Cancelled;
        self.parts.mailboxes.remove(team_id);
        Ok(team.clone())
    }
    pub fn send_message(
        &mut self,
        team_id: &str,
        from_agent_id: impl Into<String>,
        to_agent_id: Option<String>,
        body: impl Into<String>,
    ) -> Result<TeamMessage, TeamRegistryError> {
        let from = from_agent_id.into();
        let from = from.trim();
        self.require_member(team_id, from)?;
        let to = to_agent_id.map(|s| s.trim().to_owned());
        if let Some(to) = &to {
            self.require_member(team_id, to)?;
        }
        let body = body.into();
        if body.trim().is_empty() {
            return Err(TeamRegistryError::EmptyMessageBody);
        }
        if body.len() > 64 * 1024 || body.contains('\0') {
            return Err(TeamRegistryError::InvalidText);
        }
        let recipients = match &to {
            Some(id) => vec![id.clone()],
            None => self
                .team(team_id)?
                .members
                .iter()
                .map(|m| m.agent_id.clone())
                .collect(),
        };
        let body = crate::redact::redact_artifact_text(&body);
        // ponytail: bounded in-memory mailboxes; scan at most 4096 envelopes to enforce the 4 MiB body limit.
        let queued = self.parts.mailboxes.values().map(Vec::len).sum::<usize>();
        let bytes = self
            .parts
            .mailboxes
            .values()
            .flatten()
            .map(|m| m.body.len())
            .sum::<usize>();
        if queued + recipients.len() > 4096
            || bytes + body.len() * recipients.len() > 4 * 1024 * 1024
        {
            return Err(TeamRegistryError::Capacity);
        }
        let seq = self
            .parts
            .next_message_seq
            .checked_add(1)
            .ok_or(TeamRegistryError::Capacity)?;
        let message = TeamMessage {
            message_id: format!("msg_{seq}"),
            team_id: team_id.into(),
            from_agent_id: from.into(),
            to_agent_id: to,
            body,
            seq,
        };
        let mailbox = self.parts.mailboxes.entry(team_id.into()).or_default();
        for recipient in recipients {
            mailbox.push(TeamMessage {
                to_agent_id: Some(recipient),
                ..message.clone()
            });
        }
        self.parts.next_message_seq = seq;
        Ok(message)
    }
    pub fn peek_inbox(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TeamMessage>, TeamRegistryError> {
        self.require_member(team_id, agent_id)?;
        Ok(self
            .parts
            .mailboxes
            .get(team_id)
            .into_iter()
            .flatten()
            .filter(|m| is_recipient(m, agent_id))
            .cloned()
            .collect())
    }
    pub fn receive_messages(
        &mut self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TeamMessage>, TeamRegistryError> {
        self.require_member(team_id, agent_id)?;
        let Some(mailbox) = self.parts.mailboxes.get_mut(team_id) else {
            return Ok(Vec::new());
        };
        let (received, retained) = std::mem::take(mailbox)
            .into_iter()
            .partition(|m| is_recipient(m, agent_id));
        *mailbox = retained;
        Ok(received)
    }
    pub fn mailbox_len(&self, team_id: &str) -> Result<usize, TeamRegistryError> {
        self.team(team_id)?;
        Ok(self.parts.mailboxes.get(team_id).map_or(0, Vec::len))
    }
    pub fn summary(&self) -> TeamRegistrySummary {
        let teams = self.parts.teams.len();
        let active = self
            .parts
            .teams
            .values()
            .filter(|t| t.status == TeamStatus::Active)
            .count();
        TeamRegistrySummary {
            teams,
            active,
            cancelled: teams - active,
            members: self.parts.teams.values().map(|t| t.members.len()).sum(),
            mailbox_messages: self.parts.mailboxes.values().map(Vec::len).sum(),
        }
    }
    fn team(&self, id: &str) -> Result<&TeamRecord, TeamRegistryError> {
        self.parts
            .teams
            .get(id)
            .ok_or_else(|| TeamRegistryError::NotFound { team_id: id.into() })
    }
    fn active(&mut self, id: &str) -> Result<&mut TeamRecord, TeamRegistryError> {
        let team = self
            .parts
            .teams
            .get_mut(id)
            .ok_or_else(|| TeamRegistryError::NotFound { team_id: id.into() })?;
        if team.status == TeamStatus::Cancelled {
            return Err(TeamRegistryError::Cancelled { team_id: id.into() });
        }
        Ok(team)
    }
    fn require_member(&self, team_id: &str, agent_id: &str) -> Result<(), TeamRegistryError> {
        if agent_id.is_empty() {
            return Err(TeamRegistryError::EmptyAgentId);
        }
        let team = self.team(team_id)?;
        if team.status == TeamStatus::Cancelled {
            return Err(TeamRegistryError::Cancelled {
                team_id: team_id.into(),
            });
        }
        if !team.members.iter().any(|m| m.agent_id == agent_id) {
            return Err(TeamRegistryError::NotAMember {
                team_id: team_id.into(),
                agent_id: agent_id.into(),
            });
        }
        Ok(())
    }
}
fn check_label(value: &str) -> Result<(), TeamRegistryError> {
    if value.len() > 256
        || value.chars().any(char::is_control)
        || crate::redact::redact_artifact_text(value) != value
    {
        Err(TeamRegistryError::InvalidText)
    } else {
        Ok(())
    }
}
fn is_recipient(message: &TeamMessage, agent_id: &str) -> bool {
    message
        .to_agent_id
        .as_deref()
        .is_none_or(|to| to == agent_id)
}
