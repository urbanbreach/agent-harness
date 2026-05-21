---
name: autoresearch-goal
description: Durable research workflow with explicit objective, validator, evidence checkpoints, and completion gate.
tools: [read, grep, websearch, webfetch, task, workflow_status, workflow_evidence]
permissions:
  read: allow
  grep: allow
  websearch: ask
  webfetch: ask
  task: ask
---

# Autoresearch goal

Use this skill for research that needs a durable objective, validator criteria, and evidence-backed completion.

## Loop
1. Define the research objective, scope, exclusions, and validator question.
2. Gather primary sources first: repo artifacts for local claims, official/upstream sources for external claims.
3. Record checkpoints as workflow evidence with source links, dates, and confidence.
4. Run a critic/validator pass against the objective.
5. Complete only when the validator criteria are satisfied or a concrete blocker is recorded.

## Boundary

- Do not revive deprecated direct launch surfaces or external state files as workflow authority.
- Do not claim shell commands mutate hidden goal state.
- Use Harness workflow events, projections, and evidence as the durable source of truth.

## Artifacts

Harness records mission, rubric, ledger, validation, and completion details as workflow evidence artifacts and replay-derived dossier entries.

## Flow

1. Create the mission objective and validation rubric in Harness workflow state.
2. Emit a model-facing handoff that cites the active workflow id and evidence expectations.
3. Research iteratively against the rubric and record every critic outcome as evidence.
4. Complete only after passing validation and replay-derived status/dossier checks.

## Completion gate

A passing validator artifact and matching Harness workflow projection are required. Assistant prose, partial tests, or failed/blocked verdicts are not sufficient.

## Output
Return a concise answer with citations/evidence, unresolved unknowns, and the workflow evidence identifier when available.

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
