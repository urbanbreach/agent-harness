---
name: visual-verdict
description: Structured visual QA verdict comparing reference and current screenshots with score, differences, suggestions, and evidence.
tools: [read, bash, task]
permissions:
  read: allow
  bash: ask
  task: ask
---

# Visual verdict

Use this skill to compare a reference image/state against an implementation screenshot.

## Required verdict shape
- `score` from 0-100.
- `verdict`: pass/fail/watch.
- `category_match`.
- `differences[]` with concrete visual deltas.
- `suggestions[]` that map deltas to likely edits.
- `reasoning` and artifact paths.

A score below the agreed threshold must feed the next edit plan. Pixel diffs can support diagnosis but do not replace the structured verdict.

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
