use crate::{store, team_registry::*};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
mod product;
pub use product::*;
pub const TEAM_MAILBOX_JOURNAL_REL: &str = ".agent-harness/team-mailbox.json";
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum TeamMailboxJournalError {
    #[error("team mailbox I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("team mailbox has invalid JSON or duplicate identifiers")]
    Invalid,
    #[error("unsupported team mailbox version {version} at {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error(transparent)]
    Registry(#[from] TeamRegistryError),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    next_seq: u64,
    next_message_seq: u64,
    teams: Vec<TeamRecord>,
    mailboxes: Vec<Mailbox>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mailbox {
    team_id: String,
    messages: Vec<TeamMessage>,
}
#[derive(Debug)]
pub struct DurableTeamRegistry {
    workspace_root: PathBuf,
    journal_path: PathBuf,
    registry: TeamRegistry,
}
impl DurableTeamRegistry {
    /// Reading a missing mailbox does not create workspace files.
    pub fn open(workspace_root: impl Into<PathBuf>) -> Result<Self, TeamMailboxJournalError> {
        let workspace_root = workspace_root.into();
        let journal_path = workspace_root.join(TEAM_MAILBOX_JOURNAL_REL);
        Ok(Self {
            registry: load(&journal_path)?,
            workspace_root,
            journal_path,
        })
    }
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }
    /// Last successfully loaded or committed snapshot; inbox reads load current state.
    pub fn registry(&self) -> &TeamRegistry {
        &self.registry
    }
    pub fn summary(&self) -> TeamRegistrySummary {
        self.registry.summary()
    }
    pub fn create_team(
        &mut self,
        name: impl Into<String>,
    ) -> Result<TeamRecord, TeamMailboxJournalError> {
        self.update(|r| r.create_team(name))
    }
    pub fn add_member(
        &mut self,
        team: &str,
        agent: impl Into<String>,
        role: impl Into<String>,
    ) -> Result<TeamRecord, TeamMailboxJournalError> {
        self.update(|r| r.add_member(team, agent, role))
    }
    pub fn remove_member(
        &mut self,
        team: &str,
        agent: impl Into<String>,
    ) -> Result<TeamRecord, TeamMailboxJournalError> {
        self.update(|r| r.remove_member(team, agent))
    }
    pub fn send_message(
        &mut self,
        team: &str,
        from: impl Into<String>,
        to: Option<String>,
        body: impl Into<String>,
    ) -> Result<TeamMessage, TeamMailboxJournalError> {
        self.update(|r| r.send_message(team, from, to, body))
    }
    pub fn deliver_messages(
        &mut self,
        team: &str,
        agent: &str,
    ) -> Result<Vec<TeamMessage>, TeamMailboxJournalError> {
        self.update(|r| r.receive_messages(team, agent))
    }
    pub fn cancel_team(&mut self, team: &str) -> Result<TeamRecord, TeamMailboxJournalError> {
        self.update(|r| r.cancel_team(team))
    }
    pub fn peek_inbox(
        &self,
        team: &str,
        agent: &str,
    ) -> Result<Vec<TeamMessage>, TeamMailboxJournalError> {
        Ok(load(&self.journal_path)?.peek_inbox(team, agent)?)
    }
    fn update<T>(
        &mut self,
        change: impl FnOnce(&mut TeamRegistry) -> Result<T, TeamRegistryError>,
    ) -> Result<T, TeamMailboxJournalError> {
        let _lock = store::lock_private_parent(&self.journal_path)?;
        let mut next = load(&self.journal_path)?;
        let result = change(&mut next)?;
        next.validate()?;
        let parts = next.to_parts();
        let doc = Document {
            version: 1,
            next_seq: parts.next_seq,
            next_message_seq: parts.next_message_seq,
            teams: parts.teams.into_values().collect(),
            mailboxes: parts
                .mailboxes
                .into_iter()
                .map(|(team_id, messages)| Mailbox { team_id, messages })
                .collect(),
        };
        let bytes = serde_json::to_vec(&doc).map_err(|_| TeamMailboxJournalError::Invalid)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(TeamRegistryError::Capacity.into());
        }
        store::write_private_atomic(&self.journal_path, &bytes)?;
        self.registry = next;
        Ok(result)
    }
}
fn load(path: &Path) -> Result<TeamRegistry, TeamMailboxJournalError> {
    let Some(bytes) = store::read_private_bytes(path, MAX_BYTES)? else {
        return Ok(TeamRegistry::new());
    };
    let doc: Document =
        serde_json::from_slice(&bytes).map_err(|_| TeamMailboxJournalError::Invalid)?;
    if doc.version != 1 {
        return Err(TeamMailboxJournalError::UnsupportedVersion {
            path: path.display().to_string(),
            version: doc.version,
        });
    }
    let mut parts = TeamRegistryParts {
        next_seq: doc.next_seq,
        next_message_seq: doc.next_message_seq,
        ..Default::default()
    };
    for team in doc.teams {
        if parts.teams.insert(team.team_id.clone(), team).is_some() {
            return Err(TeamMailboxJournalError::Invalid);
        }
    }
    for mailbox in doc.mailboxes {
        if parts
            .mailboxes
            .insert(mailbox.team_id, mailbox.messages)
            .is_some()
        {
            return Err(TeamMailboxJournalError::Invalid);
        }
    }
    let registry = TeamRegistry::from_parts(parts);
    registry.validate()?;
    let mut parts = registry.into_parts();
    let (mut count, mut bytes) = (0, 0);
    for (team_id, messages) in &mut parts.mailboxes {
        // Old journals stored one broadcast envelope. Expand it once so every member can consume it.
        let team = parts
            .teams
            .get(team_id)
            .ok_or(TeamMailboxJournalError::Invalid)?;
        let mut expanded = Vec::new();
        for mut message in messages.drain(..) {
            message.body = crate::redact::redact_artifact_text(&message.body);
            let copies = if message.to_agent_id.is_some() {
                1
            } else {
                team.members.len()
            };
            count += copies;
            bytes += message.body.len() * copies;
            if count > 4096 || bytes > 4 * 1024 * 1024 {
                return Err(TeamRegistryError::Capacity.into());
            }
            if message.to_agent_id.is_some() {
                expanded.push(message);
            } else {
                for member in &team.members {
                    expanded.push(TeamMessage {
                        to_agent_id: Some(member.agent_id.clone()),
                        ..message.clone()
                    });
                }
            }
        }
        *messages = expanded;
    }
    let registry = TeamRegistry::from_parts(parts);
    registry.validate()?;
    Ok(registry)
}
