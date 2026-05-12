# Phase 3: TUI And Resume UX

Use this file as a loose implementation prompt for an agent. This phase assumes Phase 1 has stable event-sourced task state and Phase 2 has a clear task/profile contract.

## Task

Expose orchestration state cleanly in replay, resume, and the Ratatui TUI without moving runtime decisions out of `harness-core`.

## Expected Outcome

- Parent sessions show child-agent/task lifecycle clearly.
- Background tasks have discoverable status, completion, failure, cancellation, and next-action hints.
- Resume flows can continue known child sessions using replay-derived state.
- TUI changes are presentation-only and derive from projections/events.

## Required Context

Read these before editing:

- `AGENTS.md`
- `crates/harness-core/AGENTS.md`
- `crates/harness-tui/AGENTS.md`
- `docs/architecture.md`
- `docs/testing.md`
- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-core/src/session_lineage.rs`
- `crates/harness-tui/src/app.rs`
- Relevant TUI view-model, transcript, layout, and theme modules discovered from `crates/harness-tui/AGENTS.md`.

## Must Do

- Treat event projections as the source of truth for all UI state.
- Show child-agent relationships using existing lineage metadata before inventing a new hierarchy.
- Distinguish queued, running, completed, failed, cancelled, late-result, and timed-out task states if the projection supports them.
- Make background completion visible without requiring agents to poll blindly.
- Preserve transcript-first and compose-first TUI contracts.
- Keep runtime state transitions in core; TUI may request actions but must not decide lifecycle state.
- Add projection tests before or alongside TUI rendering changes.
- Keep headless/replay output consistent with TUI-visible state where practical.

## Must Not Do

- Do not add runtime scheduling logic to `harness-tui`.
- Do not read task state from ad hoc files when it exists in the event log.
- Do not add visual-only state that cannot be reconstructed on replay.
- Do not conflate PTY, native screenshot, and live signoff artifacts.
- Do not introduce broad UI redesign beyond orchestration visibility.

## Likely Files

- `crates/harness-core/src/proj.rs`
- `crates/harness-core/src/transcript_projection.rs`
- `crates/harness-core/src/session_lineage.rs`
- `crates/harness/src/replay.rs`
- `crates/harness/src/sessions.rs`
- `crates/harness-tui/src/app.rs`
- `crates/harness-tui/src/transcript*`
- `crates/harness-tui/src/layout*`
- `crates/harness-tui/src/theme*`
- `docs/testing.md`

## Suggested Implementation Steps

1. Add or update projection fixtures for child-agent task lifecycle states.
2. Expose a small orchestration view model from existing projections.
3. Render child task state in transcript/operator-sidebar surfaces with minimal visual churn.
4. Add resume affordances that surface `task_id`, `session_id`, and next actions from projection data.
5. Verify replay output and TUI state agree for the same event log.
6. Update docs with the user-visible behavior and signoff expectations.

## Verification

Run projection and TUI checks first:

```bash
cargo test -p harness-core proj
cargo test -p harness-core transcript_projection
cargo test -p harness-tui
cargo check --workspace
```

For user-visible TUI changes, run the narrowest deterministic signoff lane that applies and record any artifact limitations in the final handoff.
