//! The coordinator side of the `context_notes` and `new_context` tools and of the
//! `history://current/full` route. Every call names the running tool task, so the owner
//! is the agent whose tool call is executing and nothing else can borrow its notebook.
use super::{is_valid, latest_notebook, NOTEBOOK_LIMIT, NOTEBOOK_VERSION};
use crate::coord::{runtime::JobKind, *};

const DISABLED: &str = "Experimental context management is disabled.";
const DISPOSED: &str =
    "Experimental context management is unavailable because this session is disposed.";
const UNAVAILABLE: &str = "Experimental context management is unavailable for this session.";
const BRANCH_CHANGED: &str =
    "Experimental context notes were not saved because the session branch changed.";
const ROUTE_DISABLED: &str =
    "history://current/full is available only when runtime.compaction.notes_windows is enabled";
const ROUTE_UNBOUND: &str = "history://current/full requires a bound live session branch";

fn invalid(message: &str) -> CoordinatorError {
    CoordinatorError::Invalid(message.to_owned())
}

/// What a save captured when it began; the append is refused if any of it changed.
#[derive(Debug, Clone)]
pub struct NotesTicket {
    agent: String,
    run_id: String,
    rewinds: usize,
}

/// The notebook a read returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesRevision {
    /// Entry id of the revision; absent when nothing was ever saved on this branch.
    pub entry_id: Option<String>,
    pub text: String,
}

/// A notebook revision that was appended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesSaved {
    pub entry_id: String,
    pub text: String,
    pub bytes: usize,
}

/// The oversize error, with the exact byte count of what was offered.
fn oversize_message(bytes: usize) -> String {
    format!(
        "Context notes are {bytes} bytes; the limit is {NOTEBOOK_LIMIT} UTF-8 bytes. Shorten the notebook and use history://current/full to recover raw detail."
    )
}

impl Runtime {
    /// Owner of the tool call, in the documented order: disabled, disposed, unavailable.
    fn notes_owner(&self, task: &str) -> Result<String, CoordinatorError> {
        if !self.config.compaction.notes_windows {
            return Err(invalid(DISABLED));
        }
        let run_id = match self.accepting().and_then(|()| self.info()) {
            Ok(info) => info.run_id.to_string(),
            Err(_) => return Err(invalid(DISPOSED)),
        };
        let agent = self
            .running
            .get(task)
            .filter(|job| matches!(job.kind, JobKind::Tool { .. }))
            .and_then(|job| job.actor.agent_id.clone())
            .filter(|id| self.agents.contains_key(id))
            .ok_or_else(|| invalid(UNAVAILABLE))?;
        if self.killed_agents.contains(&agent)
            || self.stopped_sessions.contains(&agent)
            || self.stopped_sessions.contains(&run_id)
        {
            return Err(invalid(DISPOSED));
        }
        Ok(agent)
    }

    /// The caller's active branch events and how many rewinds the journal holds.
    pub(super) fn notes_branch(&self) -> Result<(Vec<EventEnvelopeV1>, usize), CoordinatorError> {
        let events = crate::store::read_events(&self.info()?.events_path)?;
        let rewinds = events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ConversationRewound(_)))
            .count();
        let active = crate::conversation_rewind::active_events(&events).into_owned();
        Ok((active, rewinds))
    }

    /// The turn a running tool call belongs to, following nested calls up to the turn.
    fn owning_turn(&self, task: &str) -> Option<String> {
        let mut current = task;
        for _ in 0..16 {
            let job = self.running.get(current)?;
            if matches!(job.kind, JobKind::Turn { .. }) {
                return Some(current.to_owned());
            }
            current = job.parent.as_deref()?;
        }
        None
    }

    /// Records that the caller's turn asked for a fresh window. The worker acts on it once
    /// the current tool batch has finished.
    fn new_context_request(&mut self, task: &str) -> Result<(), CoordinatorError> {
        self.check_task(task)?;
        let agent = self.notes_owner(task)?;
        if let Some(turn) = self.owning_turn(task) {
            self.notes.entry(agent).or_default().new_context_turn = Some(turn);
        }
        Ok(())
    }

    fn notes_ticket(&self, task: &str) -> Result<NotesTicket, CoordinatorError> {
        self.check_task(task)?;
        let agent = self.notes_owner(task)?;
        let (_, rewinds) = self.notes_branch()?;
        Ok(NotesTicket {
            agent,
            run_id: self.info()?.run_id.to_string(),
            rewinds,
        })
    }

    fn notes_read(&self, task: &str) -> Result<NotesRevision, CoordinatorError> {
        self.check_task(task)?;
        let agent = self.notes_owner(task)?;
        let (active, _) = self.notes_branch()?;
        Ok(latest_notebook(&active, &agent).map_or_else(
            || NotesRevision {
                entry_id: None,
                text: String::new(),
            },
            |revision| NotesRevision {
                entry_id: Some(revision.seq.to_string()),
                text: revision.text,
            },
        ))
    }

    fn notes_save(
        &mut self,
        task: &str,
        ticket: &NotesTicket,
        text: String,
    ) -> Result<NotesSaved, CoordinatorError> {
        self.check_task(task)?;
        let agent = self.notes_owner(task)?;
        let (_, rewinds) = self.notes_branch()?;
        if agent != ticket.agent
            || self.info()?.run_id.as_str() != ticket.run_id
            || rewinds != ticket.rewinds
        {
            return Err(invalid(BRANCH_CHANGED));
        }
        // The stored revision is what the redactor leaves, and that is what must fit.
        let text = self.redactor.redact_text(&text);
        if text.len() > NOTEBOOK_LIMIT {
            return Err(CoordinatorError::Invalid(oversize_message(text.len())));
        }
        let note = ContextNotesEvent {
            agent_id: agent.clone(),
            version: NOTEBOOK_VERSION,
            text,
        };
        debug_assert!(is_valid(&note));
        let event = self.emit(
            EventActor::new(ActorKind::Worker, Some(agent)),
            Some(task.to_owned()),
            EventV1::ContextNotes(note.clone()),
        )?;
        Ok(NotesSaved {
            entry_id: event.seq.to_string(),
            bytes: note.text.len(),
            text: note.text,
        })
    }

    fn history_branch(&self, task: &str) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
        if !self.config.compaction.notes_windows {
            return Err(invalid(ROUTE_DISABLED));
        }
        self.check_task(task)?;
        let agent = self.notes_owner(task).map_err(|_| invalid(ROUTE_UNBOUND))?;
        let (active, _) = self.notes_branch()?;
        Ok(active
            .into_iter()
            .filter(|event| belongs_to(event, &agent, self))
            .collect())
    }
}

/// Whether an active-branch event is part of `agent`'s own history: its own records, plus
/// run-level records for the primary agent.
fn belongs_to(event: &EventEnvelopeV1, agent: &str, runtime: &Runtime) -> bool {
    let recorded_for = match &event.payload {
        EventV1::ContextNotes(note) => Some(note.agent_id.as_str()),
        EventV1::SessionCompaction(compaction) => Some(compaction.agent_id.as_str()),
        EventV1::BranchSummary(branch) => Some(branch.agent_id.as_str()),
        _ => None,
    };
    if let Some(id) = recorded_for {
        return id == agent;
    }
    match event.actor.agent_id.as_deref() {
        Some(id) => id == agent,
        None => runtime
            .agents
            .get(agent)
            .is_some_and(|a| a.info.parent_agent_id.is_none()),
    }
}

impl CoordinatorHandle {
    /// Starts a save: checks the caller may use the notebook and captures the branch.
    pub async fn context_notes_ticket(
        &self,
        task: impl Into<String>,
    ) -> Result<NotesTicket, CoordinatorError> {
        let task = task.into();
        self.call(move |s| s.notes_ticket(&task)).await
    }

    /// The caller's latest notebook revision.
    pub async fn context_notes_read(
        &self,
        task: impl Into<String>,
    ) -> Result<NotesRevision, CoordinatorError> {
        let task = task.into();
        self.call(move |s| s.notes_read(&task)).await
    }

    /// Appends a replacement revision unless the owner, the setting, the session or the
    /// branch changed since `ticket` was taken.
    pub async fn context_notes_save(
        &self,
        task: impl Into<String>,
        ticket: NotesTicket,
        text: String,
    ) -> Result<NotesSaved, CoordinatorError> {
        let task = task.into();
        self.call(move |s| s.notes_save(&task, &ticket, text)).await
    }

    /// Checks the caller may request a fresh context window and records the request for
    /// the worker of the turn it belongs to.
    pub async fn new_context_request(
        &self,
        task: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        let task = task.into();
        self.call(move |s| s.new_context_request(&task)).await
    }

    /// Every record of the caller's active branch, before and after compactions, for the
    /// `history://current/full` route.
    pub async fn current_branch_events(
        &self,
        task: impl Into<String>,
    ) -> Result<Vec<EventEnvelopeV1>, CoordinatorError> {
        let task = task.into();
        self.call(move |s| s.history_branch(&task)).await
    }
}
