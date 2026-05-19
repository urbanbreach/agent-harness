---
name: performance-goal
description: "Run an evaluator-gated performance optimization workflow over external goal context with durable Harness artifacts and safe goal handoffs."
---

# Performance Goal Workflow

Use this skill when a user asks Harness to optimize performance and wants a goal-oriented loop rather than a one-off review.

## Contract

- Harness owns durable workflow state under `.omx/goals/performance/<slug>/`.
- external goal context owns only the active-thread focus/accounting primitive.
- Shell commands do **not** mutate hidden Codex goal state. They write artifacts and emit model-facing handoff text.
- No optimization work may start until an evaluator command and pass/fail contract exist.
- Do not call `update_goal({status: "complete"})` until the evaluator has a passing checkpoint and a completion audit proves the objective is done; then call `get_goal` again and pass that fresh snapshot to `omx performance-goal complete --codex-goal-json`.

## CLI

Create the workflow and evaluator contract:

```sh
omx performance-goal create \
  --objective "Reduce CLI startup latency by 20%" \
  --evaluator-command "npm run perf:startup" \
  --evaluator-contract "PASS when p95 latency improves by 20% and regression tests pass" \
  --slug startup-latency
```

Emit the Codex goal handoff:

```sh
omx performance-goal start --slug startup-latency
```

Record evaluator evidence:

```sh
omx performance-goal checkpoint --slug startup-latency --status pass --evidence "benchmark + tests passed"
omx performance-goal checkpoint --slug startup-latency --status fail --evidence "benchmark regressed"
omx performance-goal checkpoint --slug startup-latency --status blocked --evidence "missing fixture"
```

Complete only after a passing checkpoint:

```sh
omx performance-goal complete --slug startup-latency --evidence "final evaluator evidence" --codex-goal-json <get_goal-json-or-path>
```

## Agent Loop

1. Run `omx performance-goal create` if no workflow exists.
2. Run `omx performance-goal start` and follow the handoff:
   - call `get_goal`;
   - call `create_goal` only when no active goal exists and the objective is explicit;
   - work only against the evaluator contract;
   - after evaluator pass and completion audit, call `update_goal({status: "complete"})`, call `get_goal` again, and pass that snapshot to `omx performance-goal complete --codex-goal-json`;
3. Optimize in small reversible patches.
4. Run the evaluator and related regression tests.
5. Record each pass/fail/blocker with `checkpoint`.
6. Complete only when the pass artifact exists and no required work remains.

## Completion Gate

A performance goal is incomplete unless `.omx/goals/performance/<slug>/state.json` contains a `lastValidation.status` of `pass` and `omx performance-goal complete` receives a matching complete Codex `get_goal` snapshot via `--codex-goal-json`. Passing ordinary tests alone is not sufficient unless they are the declared evaluator contract.

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
