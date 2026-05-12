# Phase 4: Team Orchestration

Use this file as a loose implementation prompt for an agent. This phase should not start until single child-agent orchestration, resolver/config behavior, and TUI/replay visibility are stable.

## Task

Design and implement event-sourced team orchestration for coordinated multi-agent work: team specs, member lifecycle, shared messages, shared task list, shutdown protocol, and optional isolation surfaces.

## Expected Outcome

- Teams are represented as first-class event-sourced runtime state.
- Team members are ordinary child agents scheduled by the coordinator.
- Team messages, task claims, task completion, and shutdown requests are replayable.
- The tool surface is explicit enough for lead agents and team members to coordinate without out-of-band state.

## Required Context

Read these before editing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tools/AGENTS.md`
- `crates/harness-tui/AGENTS.md`
- `docs/architecture.md`
- `docs/config.md`
- Phase 1, Phase 2, and Phase 3 orchestration files in this directory.
- OMO inspiration: `inspirations/oh-my-openagent/src/features/team-mode/AGENTS.md`
- OMO inspiration: `inspirations/oh-my-openagent/src/features/team-mode/types.ts`

## Inspiration To Adapt

Adapt OMO team-mode concepts, but translate them into the harness event model:

- `TeamSpec` with version, name, description, lead, members, and bounds.
- Members that resolve to category/profile or direct subagent/profile selection.
- Runtime state with team run id, member session ids, member statuses, and bounds.
- Shared message model with `from`, `to`, `kind`, `body`, `summary`, references, timestamp, and correlation id.
- Shared task list with status, owner, blockers, metadata, and timestamps.
- Shutdown request, approval, rejection, and final deletion lifecycle.

## Must Do

- Start with a written event model proposal before editing code.
- Keep team lifecycle authority in `harness-core::coord`.
- Represent team state through append-only events and pure projections.
- Reuse child-agent scheduling from earlier phases for team members.
- Define role eligibility in terms of existing profiles, toolsets, and permissions.
- Keep read-only agents eligible for research delegation but not for roles that require writing team mailbox/task state.
- Add strict tool schemas for team operations if adding a tool surface.
- Add replay tests for team creation, member spawn, message send, task claim/update, dependency blocking, shutdown request, approval/rejection, and deletion.
- Consider optional worktree/tmux isolation only after the event model and core team lifecycle are stable.

## Must Not Do

- Do not implement teams as filesystem mailboxes outside the event log.
- Do not spawn detached subprocesses for team members.
- Do not make TUI or tools own team lifecycle state.
- Do not add tmux or worktree requirements to the MVP team model.
- Do not add new dependencies.
- Do not reuse OMO's TypeScript schemas mechanically; translate the concepts to Rust types and harness invariants.

## Likely Files

- `crates/harness-core/src/event.rs`
- `crates/harness-core/src/coord.rs`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-core/src/config.rs`
- `crates/harness-tools/src/lib.rs`
- New or existing `crates/harness-tools/src/*team*` module
- `crates/harness-tui/src/*` for presentation after core state exists
- `configs/harness.example.jsonc`
- `docs/architecture.md`
- `docs/config.md`
- `docs/testing.md`

## Suggested Implementation Steps

1. Draft event types and projection state for team lifecycle without implementing tools yet.
2. Add pure projection tests for the proposed team events.
3. Implement coordinator APIs for team creation, member spawn, message append, task mutation, and shutdown state transitions.
4. Add thin native tools that call those coordinator APIs.
5. Add prompt/tool descriptions for lead and member coordination.
6. Add TUI/replay rendering after core events and tools are stable.
7. Revisit optional worktree/tmux isolation as a separate follow-up.

## Verification

Run the narrowest checks for each layer touched:

```bash
cargo test -p harness-core team
cargo test -p harness-tools team
cargo test -p harness-tools --test native_tool_parity_matrix
cargo test -p harness-tui
cargo check --workspace
```

If public events, config, or docs change, also run the relevant architecture/config drift tests and update generated schemas through the established project workflow.
