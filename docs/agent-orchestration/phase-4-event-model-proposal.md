# Phase 4 event model proposal

Team orchestration is represented only through append-only runtime events owned by
`harness-core::coord`:

- `TeamCreated` stores the versioned team spec, member selectors, and run bounds.
- `TeamMemberSpawned` links a team member name to an ordinary coordinator-spawned
  child agent session.
- `TeamMessageSent` records shared messages, announcements, and references. The
  envelope timestamp is the message timestamp.
- `TeamTaskCreated` / `TeamTaskUpdated` record shared checklist state separate
  from scheduler `TaskScheduled` work. The event envelope timestamp is the task
  create/update timestamp.
- `TeamShutdownRequested` / `TeamShutdownApproved` / `TeamShutdownRejected` /
  `TeamDeleted` record the replayable shutdown protocol.

Coordinator validation preflights member resolution before the first team event,
binds worker mutations to their projected member identity, enforces team bounds,
and keeps tools/TUI/replay as projection consumers rather than lifecycle owners.
