//! Durable multi-agent team mailbox journal under a workspace.
//!
//! Persists team registry + mailbox at `.agent-harness/team-mailbox.json`.
//! Delivers messages via fail-closed membership rules with reload across restarts.

mod product;
mod store;

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::team_registry::{
    TeamMessage, TeamRecord, TeamRegistry, TeamRegistryError, TeamRegistryParts,
    TeamRegistrySummary,
};

use store::{load_or_empty, save, MailboxBucket, TeamMailboxDocument};

pub use product::{run_durable_multi_agent_team_product, MultiAgentTeamProduct};

/// Relative durable store path under a workspace root.
pub const TEAM_MAILBOX_JOURNAL_REL: &str = ".agent-harness/team-mailbox.json";

pub(crate) const STORE_VERSION: u32 = 1;

/// Failures for durable team mailbox I/O.
#[derive(Debug, Error)]
pub enum TeamMailboxJournalError {
    #[error("failed to create team-mailbox parent directory {path}: {source}")]
    CreateParent {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to read team-mailbox journal {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse team-mailbox journal {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("unsupported team-mailbox journal version {version} at {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("failed to write team-mailbox journal {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to replace team-mailbox journal {path}: {source}")]
    Replace {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Registry(#[from] TeamRegistryError),
}

/// Durable team registry + mailbox for one workspace.
#[derive(Debug, Clone)]
pub struct DurableTeamRegistry {
    workspace_root: PathBuf,
    journal_path: PathBuf,
    pub(crate) registry: TeamRegistry,
    next_seq: u64,
    next_message_seq: u64,
}

impl DurableTeamRegistry {
    pub fn open(workspace_root: impl Into<PathBuf>) -> Result<Self, TeamMailboxJournalError> {
        let workspace_root = workspace_root.into();
        let journal_path = workspace_root.join(TEAM_MAILBOX_JOURNAL_REL);
        let doc = load_or_empty(&journal_path)?;
        let registry = team_registry_from_document(&doc);
        Ok(Self {
            workspace_root,
            journal_path,
            registry,
            next_seq: doc.next_seq,
            next_message_seq: doc.next_message_seq,
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

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
        let record = self.registry.create_team(name)?;
        self.persist()?;
        Ok(record)
    }

    pub fn add_member(
        &mut self,
        team_id: &str,
        agent_id: impl Into<String>,
        role: impl Into<String>,
    ) -> Result<TeamRecord, TeamMailboxJournalError> {
        let record = self.registry.add_member(team_id, agent_id, role)?;
        self.persist()?;
        Ok(record)
    }

    pub fn send_message(
        &mut self,
        team_id: &str,
        from_agent_id: impl Into<String>,
        to_agent_id: Option<String>,
        body: impl Into<String>,
    ) -> Result<TeamMessage, TeamMailboxJournalError> {
        let message = self
            .registry
            .send_message(team_id, from_agent_id, to_agent_id, body)?;
        self.persist()?;
        Ok(message)
    }

    /// Deliver (drain) undelivered mailbox messages for an agent and persist.
    pub fn deliver_messages(
        &mut self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TeamMessage>, TeamMailboxJournalError> {
        let delivered = self.registry.receive_messages(team_id, agent_id)?;
        self.persist()?;
        Ok(delivered)
    }

    pub fn cancel_team(&mut self, team_id: &str) -> Result<TeamRecord, TeamMailboxJournalError> {
        let record = self.registry.cancel_team(team_id)?;
        self.persist()?;
        Ok(record)
    }

    pub fn peek_inbox(
        &self,
        team_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TeamMessage>, TeamMailboxJournalError> {
        Ok(self.registry.peek_inbox(team_id, agent_id)?)
    }

    pub(crate) fn persist(&mut self) -> Result<(), TeamMailboxJournalError> {
        let doc = snapshot_document(&self.registry);
        self.next_seq = doc.next_seq;
        self.next_message_seq = doc.next_message_seq;
        save(&self.journal_path, &doc)
    }
}

fn team_registry_from_document(doc: &TeamMailboxDocument) -> TeamRegistry {
    let teams = doc
        .teams
        .iter()
        .cloned()
        .map(|t| (t.team_id.clone(), t))
        .collect();
    let mailboxes = doc
        .mailboxes
        .iter()
        .cloned()
        .map(|b| (b.team_id, b.messages))
        .collect();
    TeamRegistry::from_parts(TeamRegistryParts {
        teams,
        mailboxes,
        next_seq: doc.next_seq,
        next_message_seq: doc.next_message_seq,
    })
}

fn snapshot_document(registry: &TeamRegistry) -> TeamMailboxDocument {
    let parts = registry.to_parts();
    TeamMailboxDocument {
        version: STORE_VERSION,
        next_seq: parts.next_seq,
        next_message_seq: parts.next_message_seq,
        teams: parts.teams.into_values().collect(),
        mailboxes: parts
            .mailboxes
            .into_iter()
            .map(|(team_id, messages)| MailboxBucket { team_id, messages })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_team_mailbox_persists_deliver_and_reloads() {
        // Given
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path();
        let mut durable = DurableTeamRegistry::open(root).expect("open");
        let team = durable.create_team("alpha").expect("create");
        durable
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");
        durable
            .add_member(&team.team_id, "worker", "worker")
            .expect("worker");
        durable
            .send_message(&team.team_id, "lead", Some("worker".into()), "do work")
            .expect("send");
        assert!(durable.journal_path().is_file());

        // When
        let delivered = durable
            .deliver_messages(&team.team_id, "worker")
            .expect("deliver");
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].body, "do work");

        // Then
        let reloaded = DurableTeamRegistry::open(root).expect("reload");
        let peek = reloaded.peek_inbox(&team.team_id, "worker").expect("peek");
        assert!(peek.is_empty());
        assert_eq!(reloaded.summary().teams, 1);
        assert_eq!(reloaded.summary().members, 2);
    }

    #[test]
    fn durable_team_fail_closed_on_non_member_send() {
        // Given
        let temp = tempfile::tempdir().expect("temp");
        let mut durable = DurableTeamRegistry::open(temp.path()).expect("open");
        let team = durable.create_team("strict").expect("create");
        durable
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");

        // When / Then
        let err = durable
            .send_message(&team.team_id, "ghost", None, "nope")
            .expect_err("non-member");
        assert!(matches!(
            err,
            TeamMailboxJournalError::Registry(TeamRegistryError::NotAMember { .. })
        ));
    }

    #[test]
    fn durable_team_send_after_cancel_fails_closed() {
        // arrange
        let temp = tempfile::tempdir().expect("temp");
        let mut durable = DurableTeamRegistry::open(temp.path()).expect("open");
        let team = durable.create_team("winding-down").expect("create");
        durable
            .add_member(&team.team_id, "lead", "lead")
            .expect("lead");
        durable.cancel_team(&team.team_id).expect("cancel");

        // act — even a member cannot send once the team is cancelled
        let err = durable
            .send_message(&team.team_id, "lead", None, "late message")
            .expect_err("cancelled team");

        // assert
        assert!(matches!(
            err,
            TeamMailboxJournalError::Registry(TeamRegistryError::Cancelled { .. })
        ));
    }

    #[test]
    fn durable_multi_agent_team_product_meets_contract() {
        // Given
        let temp = tempfile::tempdir().expect("temp");

        // When
        let product = run_durable_multi_agent_team_product(temp.path()).expect("product");

        // Then
        assert!(
            product.meets_durable_team_contract(),
            "durable team product failed: summary={:?} delivered={} journal={}",
            product.summary,
            product.delivered_count,
            product.journal_path,
        );
        let reloaded = DurableTeamRegistry::open(temp.path()).expect("reload");
        assert!(reloaded.summary().teams >= 2);
        assert!(reloaded.summary().cancelled >= 1);
    }
}
