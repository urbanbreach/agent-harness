---
name: performance-goal
description: "Run an evaluator-gated performance optimization workflow over external goal context with durable Harness artifacts and safe goal handoffs."
---

# Performance Goal Workflow

Use this skill when a user asks Harness to optimize performance and wants a goal-oriented loop rather than a one-off review.

## Contract

- Harness owns durable workflow state through workflow goal/evidence events and replay-derived projections.
- external goal context owns only the active-thread focus/accounting primitive.
- Shell commands do **not** mutate hidden Codex goal state. They write artifacts and emit model-facing handoff text.
- No optimization work may start until an evaluator command and pass/fail contract exist.
- Do not mark a goal complete until the evaluator has a passing checkpoint and a completion audit proves the objective is done; record the fresh snapshot as Harness workflow evidence.

## CLI

Create the workflow and evaluator contract:

Use `$performance-goal` to create the evaluator contract, with an objective, evaluator command, evaluator pass/fail contract, and slug.

Emit the Codex goal handoff:

Record the start handoff as workflow evidence and include the evaluator contract in the artifact summary.

Record evaluator evidence:

Record pass/fail/blocked checkpoints as workflow evidence with the evaluator output or blocker artifact.

Complete only after a passing checkpoint:

Complete only after the passing evaluator checkpoint and final audit are recorded as Harness evidence.

## Agent Loop

1. Start `$performance-goal` with an evaluator contract if no workflow exists.
2. Record the start handoff and follow it:
   - call `get_goal`;
   - call `create_goal` only when no active goal exists and the objective is explicit;
   - work only against the evaluator contract;
   - after evaluator pass and completion audit, record the final snapshot as workflow evidence and then close the workflow;
3. Optimize in small reversible patches.
4. Run the evaluator and related regression tests.
5. Record each pass/fail/blocker with `checkpoint`.
6. Complete only when the pass artifact exists and no required work remains.

## Completion Gate

A performance goal is incomplete unless the replay-derived workflow projection contains a passing evaluator checkpoint and final completion evidence. Passing ordinary tests alone is not sufficient unless they are the declared evaluator contract.

## Harness substrate override

When this skill is loaded by `agent-harness`, the workflow protocol above is the behavioral source, but the runtime substrate differs from Harness:

- Use coordinator-owned workflow events, workflow projections, task records, and evidence artifacts as the authority.
- Do **not** write or mutate per-mode `Harness workflow projection/*.json` files; lifecycle, phase, continuation, and closeout state are event-sourced by the harness.
- Translate Harness CLI/state operations to harness-native surfaces when needed: workflow evidence/status/goal/wiki CLI commands, native `task`/team tools, and workflow projections.
- Treat native terminal UI-specific Harness team/question instructions as conceptual guidance unless the harness exposes an equivalent native tool; prefer the harness native tool surface.
- Keep final claims evidence-backed: changed files, commands run, artifacts/evidence refs, remaining risks, and the stop condition reached.

## Harness state contract

Harness workflow state is authoritative through coordinator-owned events, workflow projections, native tool artifacts, and recorded workflow evidence. Skills must not require external state files, terminal-pane routing, or upstream CLI lifecycle commands as proof of progress.

## Execution protocol

Use the native Harness command dispatch, question, team, task, evidence, and verification surfaces named by the active workflow. Treat compatibility references as historical context only, and translate them into coordinator-owned actions before acting.

## Evidence and closeout contract

Record material progress as workflow evidence with artifact paths or command output summaries. Close only after the relevant checks pass, pending tasks are resolved or explicitly aborted, and the operator-facing status can be replayed from Harness events.

## Stop/escalation conditions

Stop when the workflow objective is verified complete, cancelled by the operator, or blocked by missing authority. Escalate only for destructive, credentialed, external-production, or materially scope-changing choices.

## Verification checklist

- Native Harness workflow projection reflects the expected mode/status.
- Required evidence artifacts or command summaries are recorded.
- Targeted tests, lint, docs checks, or visual/review gates named by the workflow have fresh results.
- No external state-file, terminal multiplexer, or upstream CLI command is the proof boundary.

## Purpose

Provide a native Harness workflow protocol for this skill so command dispatch, state projection, evidence, and closeout remain coordinator-owned and replayable.

## Use when

Use this skill when the matching `$` workflow command or catalog entry is selected and the operator request fits the workflow description.
