# Phase 5: Orchestration Hardening And Signoff

Use this file as a loose implementation prompt for an agent. This phase assumes
Phases 1-4 are implemented and focuses on closing the remaining correctness,
replay, and operator-experience gaps before treating orchestration as a stable
runtime surface.

## Task

Harden the implemented child-agent and team orchestration lanes so team state,
task state, shutdown state, replay/resume state, and TUI-visible state all obey
the documented event-sourced contracts under realistic use.

## Expected Outcome

- Team bounds are meaningful runtime limits, not just validated config fields.
- Team lead, member, read-only research, and write-capable coordination roles are
  explicit, projected, permissioned, and tested.
- Shutdown approval changes what a member can do, and deletion remains a final
  replayable lifecycle step.
- Team messages, shared tasks, shutdown decisions, and child-agent lifecycle are
  visible through replay/TUI surfaces derived from events.
- `task`, `background_output`, and `team_*` remain thin tool adapters over
  coordinator-owned APIs.
- Public docs, schemas, prompt/tool descriptions, and tests describe the same
  orchestration contract.

## Implemented Stable Role And Bounds Contract

- **Supervisor/operator**: the human/coordinator-side caller may create teams,
  inspect status, request shutdown, approve deletion preconditions, and perform
  lead-style coordination when no spawned lead is configured.
- **Lead**: `TeamSpec.lead` is a write-capable selector resolved before
  `TeamCreated`. When present, the coordinator spawns it as a first-class lead
  runtime (`TeamMemberSpawned.member_name = "lead"`) and projects its profile,
  agent id, and status separately from ordinary members. Read-only/planning
  profiles are invalid as leads.
- **Write-capable member**: the default `TeamMemberSpec.role = "member"`. These
  members may write team messages and shared checklist tasks while active, but
  not after shutdown approval. Read-only/planning profiles are invalid for this
  role.
- **Research member**: `TeamMemberSpec.role = "research"`. This role is for
  read-only profiles such as `explore`; the coordinator allows it to join and be
  projected as team state, but blocks mailbox/task mutations. It may still make
  the narrow shutdown request/approval needed to close its lifecycle.

Bounds are coordinator-enforced before appending write events. The coordinator
spawns the lead plus at most `max_parallel_members` ordinary members at create
time; pending members activate deterministically after a running member reaches
shutdown-approved. `max_member_turns` counts non-shutdown member writes from the
event log and blocks additional mailbox/task work. `max_wall_clock_minutes`
blocks non-shutdown team writes after the deadline while still allowing shutdown
and deletion cleanup.

## Required Context

Read these before editing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tools/AGENTS.md`
- `crates/harness-tui/AGENTS.md`
- `docs/architecture.md`
- `docs/config.md`
- `docs/testing.md`
- All Phase 1-4 orchestration files in this directory
- `crates/harness-core/src/event.rs`
- `crates/harness-core/src/coord.rs`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-tools/src/team_ops.rs`
- Relevant TUI projection/rendering modules under `crates/harness-tui/src/app/`
  and `crates/harness-tui/src/ui*`

## Known Gaps To Close

- `TeamBounds.max_parallel_members`, `max_wall_clock_minutes`, and
  `max_member_turns` are accepted and range-validated, but not enforced as
  runtime behavior.
- Shutdown-approved team members are projected as approved but can still mutate
  team mailbox/task state until the whole team is deleted.
- Read-only research profiles are rejected as team members instead of being
  allowed for read-only research roles and blocked only from write-capable team
  mailbox/task mutations.
- `TeamSpec.lead` is accepted, but the lead selector is not resolved,
  preflighted, spawned, permissioned, or projected as a first-class lead role.
- TUI replay suppresses orchestration status summaries, and transcript projection
  currently drops all `Team*` events instead of surfacing replayable team
  messages, task mutations, and shutdown decisions.

## Must Do

- Decide and document the minimal stable role model before editing code:
  - lead role
  - write-capable member role
  - read-only research member role
  - supervisor/operator role
- Preserve coordinator ownership for all lifecycle decisions. Tools and TUI may
  request actions and render projections; they must not decide lifecycle state.
- Enforce team bounds through coordinator-owned state transitions:
  - `max_parallel_members` must limit concurrently running team-member work or
    member activation in a deterministic, replayable way.
  - `max_wall_clock_minutes` must produce an observable terminal/blocked state or
    explicit policy error once exceeded.
  - `max_member_turns` must count member turns from events and reject or stop
    additional member work once exceeded.
- Make shutdown approval actionable:
  - approved members must not send messages, claim/update tasks, approve/reject
    other shutdowns, or otherwise write team state unless the role model
    explicitly allows a narrow final acknowledgement.
  - team deletion must still require the documented shutdown approval condition.
  - deletion must not silently cancel unrelated provider/tool tasks unless that
    behavior is explicitly modeled and documented.
- Finish lead semantics:
  - resolve and preflight `TeamSpec.lead` before the first team event.
  - project lead identity and runtime/session metadata.
  - make lead permissions distinguishable from member permissions.
  - ensure missing, unknown, or read-only/write-capability mismatched lead
    profiles fail before `TeamCreated` is appended.
- Support read-only research team participants without giving them write access:
  - allow read-only profiles where the role is research-only.
  - deny team mailbox/task mutation tools for research-only participants through
    coordinator validation and existing permission/toolset checks.
  - keep ordinary `task` delegation available for ad hoc research when full team
    membership is not needed.
- Improve projection fidelity:
  - include lead, role, member runtime status, bounds consumption, and shutdown
    status in `project_team_state`.
  - keep projections pure and reconstructable from `events.jsonl` alone.
  - reject or deterministically handle duplicate team message ids and task ids.
  - clarify whether `blocks` is a derived inverse of `blocked_by`; if it is,
    project it rather than trusting caller input.
- Improve replay/TUI visibility:
  - render team events in transcript projection or an explicit replay/team
    projection surface rather than dropping them.
  - show orchestration summaries in replay when replayed events contain child or
    team lifecycle state.
  - keep replay read-only; no replay surface may emit live team/task mutations.
  - keep the TUI transcript-first and operator-sidebar contracts intact.
- Keep tool schemas strict and stable:
  - preserve existing `task`, `background_output`, and `team_*` ids.
  - keep `deny_unknown_fields` on tool args.
  - update tool descriptions if role, shutdown, bounds, or replay behavior
    changes.
- Update public docs and examples after behavior is protected by tests.

## Must Not Do

- Do not introduce a second orchestration runtime or a filesystem mailbox.
- Do not make `harness-tools`, `harness`, or `harness-tui` own team lifecycle
  state.
- Do not spawn detached subprocesses for team members.
- Do not weaken permission checks, event strictness, schema strictness, or drift
  tests to make orchestration pass.
- Do not add tmux/worktree isolation to this phase unless the core event model and
  projection semantics are already complete and tested.
- Do not add new dependencies.
- Do not conflate scheduler tasks with shared team checklist tasks.

## Likely Files

- `crates/harness-core/src/event.rs`
- `crates/harness-core/src/coord.rs`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-core/src/perm.rs`
- `crates/harness-core/tests/team.rs`
- `crates/harness-core/tests/resume_plan.rs`
- `crates/harness-core/tests/transcript_projection.rs`
- `crates/harness-tools/src/team_ops.rs`
- `crates/harness-tools/src/agent_ops.rs`
- `crates/harness-tools/tests/team.rs`
- `crates/harness-tools/tests/native_tool_parity_matrix.rs`
- `crates/harness-tui/src/app/session_projection.rs`
- `crates/harness-tui/src/ui_chrome.rs`
- `crates/harness-tui/src/ui_secondary.rs`
- `crates/harness-tui/src/ui_transcript.rs`
- `crates/harness-tui/src/lib_tests.rs`
- `crates/harness/src/replay.rs`
- `docs/architecture.md`
- `docs/config.md`
- `docs/testing.md`
- `configs/harness.example.jsonc`

## Suggested Implementation Steps

1. Add failing tests for each known gap before changing behavior where practical.
2. Write a short role/bounds/shutdown decision note in this file or a sibling
   proposal if the current event model needs a public contract change.
3. Extend the team projection first, keeping it pure and replay-only.
4. Implement coordinator validation and state transitions for roles, bounds, and
   shutdown-gated mutations.
5. Update `team_ops.rs` only as a thin adapter over the new coordinator contract.
6. Add transcript/replay/TUI rendering from projections, not ad hoc event scans
   except where existing TUI projection code already owns event ingestion.
7. Update docs, tool descriptions, schemas, and examples only after tests lock the
   behavior.
8. Run the verification lanes below and record any intentionally deferred work.

## Verification

Run targeted checks first:

```bash
cargo test -p harness-core --test team
cargo test -p harness-core --test resume_plan
cargo test -p harness-core --test transcript_projection
cargo test -p harness-tools --test team
cargo test -p harness-tools --test native_tool_parity_matrix
cargo test -p harness-tui --lib orchestration
cargo test -p harness --test event_docs_reference
cargo test -p harness --test config_docs_reference
cargo run -p harness -- --config configs/harness.example.jsonc config validate
```

Then widen before handoff:

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

For user-visible TUI/replay changes, also run the narrowest deterministic visual
lane that applies:

```bash
scripts/test-lanes.sh signoff-pty
```

If public event or config schemas change, regenerate schema artifacts through the
existing project workflow and verify the docs drift tests pass.
